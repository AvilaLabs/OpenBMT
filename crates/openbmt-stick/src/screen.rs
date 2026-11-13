//! One screen: lines of text and, on some, a QR code.

use openbmt_qr::QrCode;

/// What the program shows at one moment.
#[derive(Debug, Clone)]
pub struct Screen {
    /// A short name for the state, for the test's serial echo.
    pub id: &'static str,
    /// The text, one line each (long lines wrap on the display).
    pub lines: Vec<String>,
    /// A QR code, drawn beside the text.
    pub qr: Option<QrCode>,
}

/// The text of a screen for the serial port (SK-40): every line, and the
/// QR matrix as rows of `#` and `.`. Present only with the `serial-echo`
/// feature, which no release has.
#[cfg(feature = "serial-echo")]
pub fn echo_text(screen: &Screen) -> String {
    let mut out = format!("OBM-ECHO BEGIN {}\n", screen.id);
    for line in &screen.lines {
        out.push_str("OBM-ECHO ");
        out.push_str(line);
        out.push('\n');
    }
    if let Some(qr) = &screen.qr {
        out.push_str(&format!("OBM-ECHO QR {} {}\n", qr.size(), qr.version()));
        for y in 0..qr.size() {
            out.push_str("OBM-ECHO QRROW ");
            for x in 0..qr.size() {
                out.push(if qr.get(x, y) { '#' } else { '.' });
            }
            out.push('\n');
        }
    }
    out.push_str("OBM-ECHO END\n");
    out
}
