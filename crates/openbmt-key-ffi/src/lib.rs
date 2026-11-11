//! The OpenBMT Key app's core as a library an Android app calls.
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
//! - Errors are one enum, [`KeyError`], with a `rule` string and a `text`
//!   (not called `message`: a Kotlin exception has a `message` of its
//!   own). A panic in any exported call is caught, becomes
//!   `KeyError::Internal` and poisons the object.
//!
//! What `KeyStore.save` receives is the `OBMx` locked frame ([`lock`]):
//! the `OBM7` envelope ([`envelope`]), which holds the identity secret and,
//! when this device holds key 2, key 2's account secret, sealed under a key
//! from the key holder's PIN (Argon2id and ChaCha20-Poly1305). A state
//! stored by an older build is the bare `OBM7` envelope: it opens only into
//! the step that sets a PIN and wraps it. Wrapping the frame with a key from
//! the Android Keystore, no backup, no device transfer, and counting the
//! wrong tries of the PIN, are the app's job and are not done here.
//!
//! Secret bytes held by this crate are overwritten when dropped where the
//! types allow it: the identity secret, key 2's account key, the words and
//! the typed rolls and words. They are not overwritten in the approver's own
//! copy of the identity (the core `Identity` type has no erase), in the
//! `bip39` mnemonic, in the byte vectors copied across the boundary,
//! or in the app's own memory.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet. Not for real funds.

// The one `unsafe` of this crate is the JNI glue; a test checks that no
// other file uses it.
#![deny(unsafe_code)]
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
mod boxinvite;
mod call;
mod ceremony;
mod codec;
mod deposit;
pub mod envelope;
mod error;
#[cfg(any(target_os = "android", feature = "jni"))]
mod jni;
mod keys;
mod kitcheck;
pub mod lock;
mod pin;
mod say;
mod screen;
mod session;
mod store;
mod views;
mod words;

#[cfg(test)]
mod tests;

pub use app::{Key2Public, KeyApp, LockStatus, SetupState, SlotView, WalletSetup, WalletView};
pub use boxinvite::InvitationView;
pub use call::{dispatch, error_response, protocol_description, Registry, MAX_APPS};
pub use ceremony::{answer_words, key3_waiting_words, Key2TakenView, Key3View};
pub use deposit::{parse_deposit_code, DepositCode};
pub use error::{KeyError, StoreError};
pub use keys::{
    coin_bits, dice_bits, dice_face_counts, BitProgress, EntropySource, KeyNetwork, KeyWordCount,
};
pub use kitcheck::KitCheck;
pub use say::{all_words, screens_markdown, words_for, Kind};
pub use screen::{AllowlistView, CeilingsView, FeeCeilingsView, SignerStatus};
pub use session::SessionHandle;
pub use store::KeyStore;
pub use views::{
    BatchDetail, BoxClaimDetail, CeilingsRemaining, CheckStatus, CheckView, Decision, Direction,
    HealthDetail, LineDetail, LineKind, LineView, OutputKindView, OutputView, OwnerLabelKind,
    OwnerLabelView, PairingDetail, SettingChangeView, SettingsDetail, SpendDetail, SpendReviewView,
};
pub use words::{word_is_valid, word_suggestions};
