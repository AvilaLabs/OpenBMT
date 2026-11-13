//! The room to receive, and what the counter does about an amount (SL-40).
//!
//! A payment bigger than the room is held by the payer for about two minutes
//! and then fails (prototype E7, `MPP_TIMEOUT_TICKS` = 3), which a customer at
//! a counter must never see. So the counter sums what its usable channels can
//! take in, takes a margin off, and offers Lightning only below that.

/// Taken off the sum before an amount is compared with it: what the
/// counterparty's reserve and a fee change between the offer and the payment
/// can eat. 1,000 sat.
pub const ROOM_MARGIN_MSAT: u64 = 1_000_000;

/// The room to receive, in millisatoshis: the sum of the inbound capacity of
/// every usable channel, less [`ROOM_MARGIN_MSAT`], never below zero.
pub fn room_to_receive_msat(inbound_capacity_msat: impl IntoIterator<Item = u64>) -> u64 {
    inbound_capacity_msat
        .into_iter()
        .fold(0u64, |sum, c| sum.saturating_add(c))
        .saturating_sub(ROOM_MARGIN_MSAT)
}

/// Why the counter shows the on-chain code alone (SL-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnChainReason {
    /// The second device is away (SL-23).
    SecondDeviceAway,
    /// The chain data is behind.
    ChainBehind,
    /// The amount is above the room and the provider cannot open a channel
    /// for it, or its fee is above the owner's limit (SL-29).
    NoRoom,
    /// The amount is zero.
    NoAmount,
}

/// What the counter does for an amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveDecision {
    /// On-chain code only.
    OnChainOnly(OnChainReason),
    /// An invoice on a channel that is open: the amount is below the room.
    Invoice,
    /// An invoice that makes the provider open a channel, at its fee (SL-29).
    NewChannel,
}

/// What the node knows when an amount is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Situation {
    /// The room to receive now.
    pub room_msat: u64,
    /// Whether the second device is reachable and the copies agree.
    pub device_ok: bool,
    /// Whether the chain data is up to date.
    pub chain_ok: bool,
    /// Whether the provider can be asked for a channel: connected, and its
    /// fee within the owner's limit for this amount.
    pub provider_can_open: bool,
}

/// The decision for `amount_msat`: below the room an invoice, above it a new
/// channel if the provider can open one, on-chain otherwise.
pub fn decide(amount_msat: u64, s: &Situation) -> ReceiveDecision {
    if amount_msat == 0 {
        return ReceiveDecision::OnChainOnly(OnChainReason::NoAmount);
    }
    if !s.device_ok {
        return ReceiveDecision::OnChainOnly(OnChainReason::SecondDeviceAway);
    }
    if !s.chain_ok {
        return ReceiveDecision::OnChainOnly(OnChainReason::ChainBehind);
    }
    if amount_msat <= s.room_msat {
        ReceiveDecision::Invoice
    } else if s.provider_can_open {
        ReceiveDecision::NewChannel
    } else {
        ReceiveDecision::OnChainOnly(OnChainReason::NoRoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn situation(room_msat: u64) -> Situation {
        Situation { room_msat, device_ok: true, chain_ok: true, provider_can_open: true }
    }

    #[test]
    fn sl_40_room_is_the_sum_of_inbound_less_the_margin() {
        assert_eq!(room_to_receive_msat([]), 0);
        assert_eq!(room_to_receive_msat([ROOM_MARGIN_MSAT]), 0);
        assert_eq!(room_to_receive_msat([10_000_000, 5_000_000]), 15_000_000 - ROOM_MARGIN_MSAT);
        assert_eq!(room_to_receive_msat([ROOM_MARGIN_MSAT - 1]), 0);
        assert_eq!(room_to_receive_msat([u64::MAX, u64::MAX]), u64::MAX - ROOM_MARGIN_MSAT);
    }

    #[test]
    fn sl_40_an_amount_up_to_the_room_is_an_invoice_above_it_is_not() {
        let s = situation(9_000_000);
        assert_eq!(decide(9_000_000, &s), ReceiveDecision::Invoice);
        assert_eq!(decide(1, &s), ReceiveDecision::Invoice);
        assert_eq!(decide(9_000_001, &s), ReceiveDecision::NewChannel);
    }

    #[test]
    fn sl_40_above_the_room_without_a_provider_it_is_on_chain() {
        let mut s = situation(9_000_000);
        s.provider_can_open = false;
        assert_eq!(decide(9_000_001, &s), ReceiveDecision::OnChainOnly(OnChainReason::NoRoom));
        assert_eq!(decide(9_000_000, &s), ReceiveDecision::Invoice);
    }

    #[test]
    fn sl_40_with_no_channel_the_room_is_zero_and_any_amount_needs_one() {
        let s = situation(room_to_receive_msat([]));
        assert_eq!(decide(1_000, &s), ReceiveDecision::NewChannel);
    }

    #[test]
    fn sl_41_the_device_away_or_the_chain_behind_shows_on_chain_alone() {
        let mut s = situation(9_000_000);
        s.device_ok = false;
        assert_eq!(decide(1_000, &s), ReceiveDecision::OnChainOnly(OnChainReason::SecondDeviceAway));
        s.device_ok = true;
        s.chain_ok = false;
        assert_eq!(decide(1_000, &s), ReceiveDecision::OnChainOnly(OnChainReason::ChainBehind));
        assert_eq!(decide(0, &situation(9_000_000)), ReceiveDecision::OnChainOnly(OnChainReason::NoAmount));
    }
}
