//! The words a person sees about Shop Lightning. Nothing else in this crate
//! is a sentence for a person.

/// Shop Lightning cannot start on this network yet (SL-60).
pub const SL_60: &str = "Shop Lightning runs only on test networks in this release, so it does not start for real bitcoin.";
/// The second device is away: the counter does not offer Lightning (SL-23, SL-41).
pub const SL_23: &str = "The second device that keeps Shop Lightning's copy is not answering, so this counter offers no Lightning until it is back";
/// The provider asks more than the owner allows for an opening (SL-29).
pub const SL_29: &str =
    "The provider asks more to open a channel than the most you allow, so no channel was opened.";
/// The provider's window to be back online is too short (SL-30).
pub const SL_30: &str = "The provider would leave the shop less than a week to catch a wrong close of a channel, so it is not used.";
/// The copy on this counter is older than the one on the second device (SL-25).
pub const SL_25: &str = "The copy of Shop Lightning on this counter is older than the one on the second device, so it is never used. Start from the second device's copy.";
/// The two copies each hold something the other lacks (SL-25).
pub const SL_25_DIVERGED: &str = "The two copies of Shop Lightning each hold something the other does not, so neither is used. Lightning stays off. Keep both devices as they are and wipe neither.";
/// The amount is above the room to receive (SL-40).
pub const SL_40: &str =
    "This amount is more than Lightning can take in right now, so it is paid on-chain.";
/// No usable channel and no provider answering (SL-41).
pub const SL_41: &str = "No Lightning channel can take a payment and the provider is not answering, so this counter shows the on-chain code alone.";
/// A channel is open (SL-45).
pub const SL_45: &str = "A Lightning channel is open.";
