//! The words a person would see. Each constant is its own name, to be
//! replaced by the words of the screen it belongs to; nothing here is a
//! sentence.

/// Shop Lightning cannot start on this network yet (SL-60).
pub const WORDS_NEEDED_SL_60: &str = "WORDS_NEEDED_SL_60";
/// The second device is away: the counter does not offer Lightning (SL-23, SL-41).
pub const WORDS_NEEDED_SL_23: &str = "WORDS_NEEDED_SL_23";
/// The provider asks more than the owner allows for an opening (SL-29).
pub const WORDS_NEEDED_SL_29: &str = "WORDS_NEEDED_SL_29";
/// The provider's window to be back online is too short (SL-30).
pub const WORDS_NEEDED_SL_30: &str = "WORDS_NEEDED_SL_30";
/// The copy on this counter is older than the one on the second device (SL-25).
pub const WORDS_NEEDED_SL_25: &str = "WORDS_NEEDED_SL_25";
/// The two copies each hold something the other lacks (SL-25).
pub const WORDS_NEEDED_SL_25_DIVERGED: &str = "WORDS_NEEDED_SL_25_DIVERGED";
/// The amount is above the room to receive (SL-40).
pub const WORDS_NEEDED_SL_40: &str = "WORDS_NEEDED_SL_40";
/// No usable channel and no provider answering (SL-41).
pub const WORDS_NEEDED_SL_41: &str = "WORDS_NEEDED_SL_41";
/// A channel is open (SL-45).
pub const WORDS_NEEDED_SL_45: &str = "WORDS_NEEDED_SL_45";
