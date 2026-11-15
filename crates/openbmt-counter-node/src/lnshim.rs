//! The few plain types of Shop Lightning's node that the rest of this crate
//! names. With the `network` feature they are the node's own; without it
//! (the build for a browser, where the node does not run) they are the same
//! plain types here, and no value of them is ever made by a node.

#[cfg(feature = "network")]
pub use openbmt_lightning_node::{funding, room, words};

/// The node's reasons and results, written out where the node is not built.
#[cfg(not(feature = "network"))]
pub mod room {
    /// Why the counter shows the on-chain code alone (SL-41).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OnChainReason {
        /// The second device is away (SL-23).
        SecondDeviceAway,
        /// The chain data is behind.
        ChainBehind,
        /// The amount is above the room (SL-29).
        NoRoom,
        /// The amount is zero.
        NoAmount,
        /// The provider is not connected or does not answer (SL-41).
        ProviderAway,
        /// The provider's fee is above the owner's limit (SL-29).
        FeeAboveMax,
        /// The provider's window to be back online is under a week (SL-30).
        WindowTooShort,
    }
}

/// What the node says of a funding, written out where the node is not built.
#[cfg(not(feature = "network"))]
pub mod funding {
    /// Why a funding is lost.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LostWhy {
        /// The channel closed before its funding was in a block (SL-27).
        Closed,
        /// No confirmation in time.
        TimedOut,
    }

    /// What became of a watched funding.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Outcome {
        /// Not yet in a block, and not yet lost.
        Watching,
        /// The first confirmation.
        Confirmed,
        /// Lost.
        Lost(LostWhy),
    }

    /// What the counter is told of a funding it was given in a paid event.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FundingEvent {
        /// The funding transaction.
        pub txid: [u8; 32],
        /// What became of it.
        pub outcome: Outcome,
    }
}

/// The node's own texts. Without the node there are none to quote.
#[cfg(not(feature = "network"))]
pub mod words {
    /// SL-60, none without the node.
    pub const SL_60: &str = "";
    /// SL-23, none without the node.
    pub const SL_23: &str = "";
    /// SL-29, none without the node.
    pub const SL_29: &str = "";
    /// SL-30, none without the node.
    pub const SL_30: &str = "";
    /// SL-40, none without the node.
    pub const SL_40: &str = "";
    /// SL-41, none without the node.
    pub const SL_41: &str = "";
    /// SL-20, none without the node.
    pub const SL_20_INVOICE_NOT_STORED: &str = "";
}
