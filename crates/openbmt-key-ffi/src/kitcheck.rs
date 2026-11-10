//! The check of a printed recovery kit by scan (`docs/DESIGN.md` §2.1 step
//! 6, §5.4, R4-2).
//!
//! Only the wallet description is compared here. The kit's other fields
//! (birthday, generations, import range, fallback descriptors) are not
//! checked by this call.

use openbmt_core::wallet::WalletDescription;

use crate::app::KeyApp;
use crate::error::{refused_by, KeyError};

/// The longest scanned text that is looked at, in bytes. A wallet
/// description is under 1,000 bytes.
const MAX_SCANNED_BYTES: usize = 8_192;

/// What the scan of the kit's description showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum KitCheck {
    /// The scanned text is the Key app's own wallet description, byte for
    /// byte (leading and trailing ASCII white space apart).
    Same,
    /// The scanned text differs from the Key app's description.
    Different {
        /// The byte position of the first difference, from 0. When one
        /// text is the start of the other, the length of the shorter.
        first_difference: u32,
        /// The scanned text is a wallet description whose four checksum
        /// words are the Key app's own. This is the R4-2 case: a
        /// description made to share the words people compare, with other
        /// keys in it. It must be shown as the most serious result, above
        /// every other difference.
        same_checksum_words: bool,
        /// The scanned text parses as a wallet description for this
        /// network.
        parses: bool,
    },
}

/// Compare `scanned` with `own`, byte for byte after trimming ASCII white
/// space at both ends of `scanned`. `words_of` gives the four checksum
/// words of a text that parses as a wallet description, and `own_words`
/// are the Key app's.
pub(crate) fn compare_kit(
    own: &str,
    own_words: &[&str; 4],
    scanned: &str,
    words_of: impl Fn(&str) -> Option<[&'static str; 4]>,
) -> KitCheck {
    let own = own.as_bytes();
    let text = scanned.as_bytes().trim_ascii();
    if text == own {
        return KitCheck::Same;
    }
    let first = own
        .iter()
        .zip(text)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| own.len().min(text.len()));
    // The text is valid UTF-8 as a whole, so a cut at ASCII white space is
    // too; this only guards the conversion.
    let parsed = std::str::from_utf8(text).ok().and_then(words_of);
    KitCheck::Different {
        first_difference: u32::try_from(first).unwrap_or(u32::MAX),
        same_checksum_words: parsed.is_some_and(|w| &w == own_words),
        parses: parsed.is_some(),
    }
}

#[uniffi::export]
impl KeyApp {
    /// `docs/DESIGN.md` §5.4: the kit is checked by scan. Compare the text
    /// of the kit's QR with this Key app's own wallet description, byte for
    /// byte, after trimming leading and trailing ASCII white space only.
    ///
    /// `Different` with `same_checksum_words` true is the R4-2 attack and
    /// the most serious result: the four words people compare match, the
    /// keys do not. Nothing changes and the scanned text is not kept. Text
    /// over 8,192 bytes is refused first (`kit-too-long`). Only the
    /// description is checked; the kit's other fields are not. Allowed in
    /// the Ready state only.
    pub fn check_kit(&self, scanned: String) -> Result<KitCheck, KeyError> {
        if scanned.len() > MAX_SCANNED_BYTES {
            return Err(refused_by(
                "kit-too-long",
                format!(
                    "the scanned text is longer than {MAX_SCANNED_BYTES} bytes: \
                     it is not a wallet description"
                ),
            ));
        }
        self.run(|inner| {
            let ready = inner.ready()?;
            let network = ready.wallet.network;
            let own = ready.approver.wallet();
            Ok(compare_kit(
                own.canonical_string(),
                &own.checksum_words(),
                &scanned,
                |t| {
                    WalletDescription::parse(t, network)
                        .ok()
                        .map(|w| w.checksum_words())
                },
            ))
        })
    }
}
