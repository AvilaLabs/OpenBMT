//! The OpenBMT owner node as a library the owner app for Android calls.
//!
//! The owner node (`openbmt-owner-node`) joins the owner's watcher to the
//! wallet's checked coin view, the approval queue, the sending of finished
//! payments, the link to the counters and the refund requests they send.
//! This crate holds no rule of its own. It converts types, keeps the node
//! on a thread of its own, and turns every state and every refusal into the
//! words a person is shown (`docs/SCREEN-RULES.md`), taken from the crates
//! that make them.
//!
//! - [`dispatch`] is the one entry point: a framed request in, a framed
//!   response out (`src/call.rs`, `bindings/PROTOCOL.md`). A [`Registry`]
//!   holds the running nodes by handle.
//! - On Android the one exported symbol is a JNI function that calls
//!   `dispatch` (`src/jni.rs`, the only file with `unsafe`).
//! - The app calls `pump` from one background thread in a loop and every
//!   other op from its screen thread. [`host`] says how they share the node
//!   and why a screen call never waits longer than a pump slice.
//! - The node's thread also holds Key 1's state, which the owner's phone
//!   keeps (`key1.bin` in the data folder), so that taking the Key app's
//!   response (`take`) is one call.
//! - Errors are one enum, [`AppError`], with a `rule`, a `text` and the
//!   `Words` of what a person is shown. A panic in any call is caught,
//!   becomes `AppError::Internal` and poisons the node.
//!
//! The data folder is the app's own (`noBackupFilesDir`). It holds the
//! owner's queue, the coin view and Key 1's state, so keeping it out of
//! backups and device transfer, and protecting the owner secret and Key 1's
//! account key the app hands over at every start, are the app's job and are
//! not done here. The folder's lock (NR-79) applies as for any other start
//! of the node.
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

mod books;
mod boxnet;
mod call;
mod ceremony;
mod codec;
mod counter_views;
mod error;
mod float;
pub mod host;
mod import_view;
#[cfg(any(target_os = "android", feature = "jni"))]
mod jni;
mod own;
mod own_spend_views;
mod pos_views;
mod price;
mod requests;
mod say;
mod settings_edit;
mod setup_lock;
mod shop_lightning;
mod signer_views;
mod starter;
mod views;
pub mod watch;
mod web_orders;

#[cfg(test)]
mod tests;

pub use boxnet::{
    BoxDeliverArgs, BoxDeliveredView, BoxFoundView, BoxLinkArgs, BoxOwnCheckArgs, BoxOwnCheckView,
    BoxOwnInvitationView, BoxQueueArgs, BoxQueuedView, PendingBoxView, WaitingBoxView,
};
pub use call::{
    dispatch, error_response, protocol_description, Registry, MAX_NODES, REQUEST_MAGIC,
    RESPONSE_MAGIC, VERSION,
};
pub use ceremony::{
    AnswerView, BuildArgs, CeremonyDefaults, FinishArgs, FinishedView, Key1Args, Key1Made,
    PairView, PartView, SetupFields, SetupStageView, WalletBuiltView,
};
pub use counter_views::{
    CounterBundleView, CounterConfirmArgs, CounterQueuedView, CounterScanView, WaitingCounterView,
};
pub use error::{AppError, Kind};
pub use float::{
    FloatCoinView, FloatCoinsView, FloatFeeArgs, FloatNodePayArgs, FloatNodeSweepArgs,
    FloatPaidView, FloatPayArgs, FloatRefillArgs, FloatRefillView, FloatSealedView, FloatSendArgs,
    FloatStatusArgs, FloatStatusView, FloatSweepArgs, FloatWalletView,
};
pub use host::{Entry, KEY1_FILE, MAX_WAIT_MS, SLICE_MS};
pub use import_view::{ImportKind, ImportView};
pub use own::{
    OwnBoxDeliveredView, OwnBoxWelcomeArgs, OwnBundleView, OwnConfirmArgs, OwnCounterArgs,
    OwnGapArgs, OwnGapView, OwnMade, OwnOfferView, OwnOwnerMade, OwnRelayArgs, OwnRequestArgs,
    OwnRequestView, OwnShopArgs, OwnWaitingView, OwnWalletView, SwitchRowView, SwitchStateKind,
};
pub use pos_views::{
    PosAckArgs, PosAlertKey, PosAlertKind, PosAlertView, PosMethodView, PosReplyArgs, PosReplyView,
    PosRequestView, PosSetupView, PosStateView, PosSubjectView, PosVendorView,
};
pub use price::{AgreedPriceView, PriceArgs, PriceReplyView, PriceSourceView};
pub use requests::{
    read as read_request, ConfirmView, DayView, PaymentAfterView, PaymentView, QuoteView,
    RequestAsk, RequestDetailView, RequestEndView, RequestEvery, RequestListView, RequestMadeView,
    RequestMakeArgs, RequestNumberArgs, RequestPriceIn, RequestRateView, RequestRepeatsView,
    RequestReturnAddedView, RequestReturnArgs, RequestRowView, RequestShareArgs, RequestSharedView,
    RequestStateView, RequestStatusView, ReturnStateView, ReturnView,
};
pub use say::{all_words, words_for};
pub use settings_edit::{
    CurrencyAction, LightningAction, SettingChangeView, SettingsEditArgs, SettingsPreviewView,
    SwapAction,
};
pub use setup_lock::PinChangeArgs;
pub use shop_lightning::{
    LightningReportView, NotOfferedView, ShopLightningAction, ShopLightningClass,
    ShopLightningEditArgs, ShopLightningHeldView, ShopLightningPreviewView, ShopLightningView,
};
pub use starter::{
    OwnRepinKind, OwnRepinView, StarterChangeKind, StarterCheckView, StarterCoinView,
    StarterFeeArgs, StarterHeldArgs, StarterImageView, StarterLateMoveArgs, StarterLateView,
    StarterLimitArgs, StarterLimitKind, StarterLimitView, StarterLineSetArgs, StarterMakeArgs,
    StarterMoveArgs, StarterNodeMoveArgs, StarterNodePayArgs, StarterOfferArgs, StarterOpenArgs,
    StarterPageArgs, StarterPaidView, StarterPayArgs, StarterSealedView, StarterSendArgs,
    StarterTypedView, StarterWalletView,
};
pub use views::{
    AlertKind, AlertView, BatchArgs, BatchWaitingView, BuiltView, CashAmountView, CashOutArgs,
    CashOutRow, ClearArgs, CounterShareView, CounterView, DeepRequestView, DeliverView,
    EntryKindView, EntryRow, FloatWatchConfig, HaltedView, HealthView, LimitView, LinkConfig,
    LinkStepView, LinkView, MergeOfferView, NoteKindView, OldAddressesView, OldKind, OutboxKind,
    OutboxRow, OutcomeKind, OutcomeView, OwnerNetwork, PumpView, QueueView, QueuedRefund,
    ReceiptsKind, ReceiptsView, RefundArgs, RefundNoteView, RefundRow, RouteView, SendKind,
    SendRow, SettingsHeldView, SettingsQueuedView, ShopCurrencyView, SkippedView, StageKind,
    StageView, StartConfig, StatusView, SwapHeldView, TakenView, ViewKind, WordsView,
};
pub use web_orders::{
    WebCallKindView, WebCallView, WebCodeView, WebHeaderView, WebOutcome, WebPairArgs, WebPairView,
    WebPairingKind, WebRefusedView, WebReplyArgs, WebStateView, WebStepArgs, WebStepKind,
    WebStepView,
};
