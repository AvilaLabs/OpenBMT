//! Reading a wallet description the owner scanned (op 145,
//! `docs/WALLET-RULES.md` WL-08 to WL-15): the key maker guide shows its
//! fingerprint and first receive address next to the key maker page's.
//!
//! The op works on no node, holds nothing and stores nothing. The text may
//! hold a private key, so it is overwritten when the call is done, and
//! neither the view nor a refusal carries any of it.

use openbmt_core::wallet_single::{ImportRefused, ImportedWallet};

use crate::codec::{wire_record, wire_unit_enum};
use crate::error::{from_words, internal, AppError, Kind};

/// Which kind of wallet the text described.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// A native SegWit single-key account.
    Single,
    /// The 2-of-3 multisig wallet.
    Multisig,
}

wire_unit_enum!(ImportKind {
    Single = 1,
    Multisig = 2
});

/// What the owner compares with the key maker page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportView {
    /// The kind of wallet.
    pub kind: ImportKind,
    /// Eight hex digits: the master fingerprint of a single-key wallet.
    /// None for a multisig description, which has no single master key.
    pub fingerprint: Option<String>,
    /// The first receive address.
    pub first_address: String,
}

wire_record!(ImportView {
    kind,
    fingerprint,
    first_address
});

/// Overwrite a text the call was given.
fn wipe(text: String) {
    let mut bytes = text.into_bytes();
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
}

fn refused(r: ImportRefused) -> AppError {
    let rule = match r {
        ImportRefused::HasPrivateKey => "WL-08",
        ImportRefused::Damaged => "WL-10",
        ImportRefused::TestNetwork => "WL-11",
        ImportRefused::Unsupported => "WL-12",
    };
    from_words(Kind::Refused, rule, r, r.words())
}

fn view(wallet: &ImportedWallet) -> Result<ImportView, AppError> {
    match wallet {
        ImportedWallet::Single(w) => Ok(ImportView {
            kind: ImportKind::Single,
            fingerprint: Some(w.fingerprint().to_string()),
            first_address: w
                .first_receive_address()
                .map_err(|e| internal(format!("the first address: {e}")))?
                .to_string(),
        }),
        ImportedWallet::Multisig(w) => Ok(ImportView {
            kind: ImportKind::Multisig,
            fingerprint: None,
            first_address: w
                .address(openbmt_core::wallet::Keychain::Receive, 0)
                .map_err(|e| internal(format!("the first address: {e}")))?
                .to_string(),
        }),
    }
}

/// Op 145: read the scanned text and give what the owner compares, or the
/// words of the refusal.
pub(crate) fn read(text: String) -> Result<ImportView, AppError> {
    let result = ImportedWallet::read(&text)
        .map_err(refused)
        .and_then(|w| view(&w));
    wipe(text);
    result
}
