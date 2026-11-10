//! The OpenBMT Key app's core as a library an Android app calls through
//! UniFFI.
//!
//! The Key app is the `Approver` of `openbmt-approver` with the signer of
//! `openbmt-signer` (`docs/SESSION-RULES.md` section 2,
//! `docs/SIGNER-RULES.md`). This crate holds no rule of its own. It converts
//! types, keeps the one approver behind a lock, and hands the new state to
//! the app's [`KeyStore`] before anything is returned. The Rust side never
//! touches the file system, the clock or a random source: the app's
//! `KeyStore` gives all three.
//!
//! - [`KeyApp`] is the one object. `KeyApp.open(store)` brings back what
//!   `store.load()` returns. Setup (`create_from_entropy`, `key_words`,
//!   `accept_wallet`, `pair_offer_bytes`) and the Key app's own screens
//!   (`signer_status`, `enroll_destination`, ceilings, marks) are methods.
//! - [`SessionHandle`] is one approval session: `lines`, `decide`, `finish`.
//!   Only one is open at a time.
//! - Errors are one enum, [`KeyError`], with a rule string and a message.
//!   A panic in any exported call is caught, becomes `KeyError::Internal`
//!   and poisons the object.
//!
//! What `KeyStore.save` receives is the `OBM7` envelope
//! ([`envelope`]): it holds the identity secret and, when this device
//! holds key 2, key 2's account secret. Protecting it at rest (a wrapping
//! key from the Android Keystore, no backup, no device transfer) is the
//! app's job and is not done here.
//!
//! Secret bytes held by this crate are overwritten when dropped where the
//! types allow it: the identity secret, key 2's account key, the words and
//! the typed rolls. They are not overwritten in the approver's own copy of
//! the identity (the core `Identity` type has no erase), in the `bip39`
//! mnemonic, in the byte vectors UniFFI copies across the boundary, or in
//! the app's own memory.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet. Not for real funds.

// UniFFI's generated scaffolding (the `extern "C"` functions and the
// pointer handling behind `#[uniffi::export]`) needs `unsafe`. The
// allowance is scoped to this crate; no hand-written line uses it, which a
// test checks.
#![allow(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod app;
pub mod envelope;
mod error;
mod keys;
mod screen;
mod session;
mod store;
mod views;

#[cfg(test)]
mod tests;

pub use app::{Key2Public, KeyApp, SetupState, SlotView, WalletSetup, WalletView};
pub use error::{KeyError, StoreError};
pub use keys::{
    coin_bits, dice_bits, dice_face_counts, BitProgress, EntropySource, KeyNetwork, KeyWordCount,
};
pub use screen::{AllowlistView, CeilingsView, FeeCeilingsView, SignerStatus};
pub use session::SessionHandle;
pub use store::KeyStore;
pub use views::{
    BatchDetail, CeilingsRemaining, CheckStatus, CheckView, Decision, Direction, HealthDetail,
    LineDetail, LineKind, LineView, OutputKindView, OutputView, OwnerLabelKind, OwnerLabelView,
    PairingDetail, SettingChangeView, SettingsDetail, SpendDetail, SpendReviewView,
};

uniffi::setup_scaffolding!();
