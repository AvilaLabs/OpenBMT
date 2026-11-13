//! The liquidity provider the shop picks, through LSPS2 (SL-28 to SL-31).
//!
//! The decisions are pure functions here, so that they are tested without a
//! provider: which of the provider's offers to take, what it costs, whether
//! its window to be back online is long enough, and what to ask for as our
//! `to_self_delay`.
//!
//! `client_trusts_lsp` is a setting of the provider's service, not of the
//! client: in `lightning-liquidity` 0.2.8 the client side has no trust option
//! and always takes the client-trusts-LSP flow of bLIP-52. The test provider
//! of `scripts/shop-lightning-e2e-core.sh` runs with it false.

use bitcoin::secp256k1::PublicKey;
use lightning_liquidity::lsps2::msgs::LSPS2OpeningFeeParams;
use lightning_liquidity::lsps2::utils::compute_opening_fee;
use std::net::SocketAddr;

/// The `to_self_delay` the node asks for: 2016 blocks, about two weeks (SL-30).
pub const WANTED_TO_SELF_DELAY: u32 = 2016;
/// A provider that allows less than this, about a week, is refused (SL-30).
pub const MIN_TO_SELF_DELAY: u32 = 1008;

/// The provider the shop chose.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Its node id. Channels are accepted from this node and no other (SL-31).
    pub node_id: PublicKey,
    /// Where to reach it.
    pub addr: SocketAddr,
    /// The token it asked for, if any.
    pub token: Option<String>,
    /// Whether a channel it opens may be used before its funding transaction
    /// has a confirmation. The provider funds the channel and the shop holds
    /// nothing in it until a payment comes; off, the first payment waits for a
    /// block.
    pub zero_conf: bool,
}

/// Why none of the provider's offers can be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The provider offered nothing.
    NoOffer,
    /// Every offer has run out.
    Expired,
    /// The amount is outside what every offer takes.
    AmountOutsideLimits,
    /// Every offer's window to be back online is under [`MIN_TO_SELF_DELAY`] (SL-30).
    WindowTooShort,
    /// The cheapest offer costs more than the owner allows for one opening (SL-29).
    FeeAboveMax {
        /// What the cheapest offer costs.
        fee_msat: u64,
    },
}

/// An offer that can be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The provider's offer, to give back to it when buying.
    pub params: LSPS2OpeningFeeParams,
    /// What the opening costs for this amount, in millisatoshis.
    pub fee_msat: u64,
    /// The `to_self_delay` to ask for (SL-30).
    pub to_self_delay: u16,
}

/// The `to_self_delay` to ask for: 2016, or the most the provider allows if
/// that is lower; `None` if that is under 1008.
pub fn to_self_delay(provider_max: u32) -> Option<u16> {
    if provider_max < MIN_TO_SELF_DELAY {
        return None;
    }
    Some(provider_max.min(WANTED_TO_SELF_DELAY) as u16)
}

/// The cheapest offer of the menu that can be taken for `amount_msat`, now
/// being `now_unix`; or why none can (SL-29, SL-30). `max_fee_msat` is what
/// the owner allows for one opening.
pub fn choose_offer(
    menu: &[LSPS2OpeningFeeParams],
    amount_msat: u64,
    max_fee_msat: u64,
    now_unix: i64,
) -> Result<Offer, Refusal> {
    if menu.is_empty() {
        return Err(Refusal::NoOffer);
    }
    let (mut expired, mut outside, mut short) = (0usize, 0usize, 0usize);
    let mut best: Option<Offer> = None;
    for p in menu {
        if p.valid_until.0.timestamp() < now_unix {
            expired += 1;
            continue;
        }
        if amount_msat < p.min_payment_size_msat || amount_msat > p.max_payment_size_msat {
            outside += 1;
            continue;
        }
        let Some(delay) = to_self_delay(p.max_client_to_self_delay) else {
            short += 1;
            continue;
        };
        let Some(fee_msat) =
            compute_opening_fee(amount_msat, p.min_fee_msat, p.proportional as u64)
        else {
            outside += 1;
            continue;
        };
        if best.as_ref().is_none_or(|b| fee_msat < b.fee_msat) {
            best = Some(Offer {
                params: p.clone(),
                fee_msat,
                to_self_delay: delay,
            });
        }
    }
    match best {
        Some(offer) if offer.fee_msat > max_fee_msat => Err(Refusal::FeeAboveMax {
            fee_msat: offer.fee_msat,
        }),
        Some(offer) => Ok(offer),
        None if short > 0 => Err(Refusal::WindowTooShort),
        None if outside > 0 => Err(Refusal::AmountOutsideLimits),
        None => {
            debug_assert!(expired > 0);
            Err(Refusal::Expired)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning_liquidity::lsps0::ser::LSPSDateTime;
    use std::time::Duration;

    const NOW: i64 = 1_800_000_000;

    fn params(min_fee_msat: u64, ppm: u32, max_delay: u32) -> LSPS2OpeningFeeParams {
        LSPS2OpeningFeeParams {
            min_fee_msat,
            proportional: ppm,
            valid_until: LSPSDateTime::new_from_duration_since_epoch(Duration::from_secs(
                NOW as u64 + 3600,
            )),
            min_lifetime: 100,
            max_client_to_self_delay: max_delay,
            min_payment_size_msat: 1_000_000,
            max_payment_size_msat: 1_000_000_000,
            promise: "p".into(),
        }
    }

    #[test]
    fn sl_30_asks_for_2016_or_the_providers_maximum_if_lower() {
        assert_eq!(to_self_delay(2016), Some(2016));
        assert_eq!(to_self_delay(10_000), Some(2016));
        assert_eq!(to_self_delay(1500), Some(1500));
        assert_eq!(to_self_delay(1008), Some(1008));
    }

    #[test]
    fn sl_30_a_provider_under_1008_is_refused() {
        assert_eq!(to_self_delay(1007), None);
        assert_eq!(to_self_delay(144), None);
        let menu = [params(1_000_000, 10_000, 144)];
        assert_eq!(
            choose_offer(&menu, 50_000_000, u64::MAX, NOW),
            Err(Refusal::WindowTooShort)
        );
    }

    #[test]
    fn sl_29_the_fee_is_the_larger_of_the_minimum_and_the_proportion_rounded_up() {
        let menu = [params(1_000_000, 10_000, 1008)];
        let o = choose_offer(&menu, 50_000_000, u64::MAX, NOW).unwrap();
        assert_eq!(o.fee_msat, 1_000_000); // 1% is 500,000 msat, the minimum is more
        let o = choose_offer(&menu, 500_000_001, u64::MAX, NOW).unwrap();
        assert_eq!(o.fee_msat, 5_000_001); // 5,000,000.01 rounds up
    }

    #[test]
    fn sl_29_a_fee_above_the_owners_maximum_is_refused() {
        let menu = [params(1_000_000, 10_000, 1008)];
        assert_eq!(
            choose_offer(&menu, 50_000_000, 999_999, NOW),
            Err(Refusal::FeeAboveMax {
                fee_msat: 1_000_000
            })
        );
        assert!(choose_offer(&menu, 50_000_000, 1_000_000, NOW).is_ok());
    }

    #[test]
    fn sl_29_the_cheapest_offer_that_can_be_taken_wins() {
        let menu = [
            params(2_000_000, 10_000, 1008),
            params(1_000_000, 10_000, 2016),
            params(1, 1, 144),
        ];
        let o = choose_offer(&menu, 50_000_000, u64::MAX, NOW).unwrap();
        assert_eq!(o.fee_msat, 1_000_000);
        assert_eq!(o.to_self_delay, 2016);
    }

    #[test]
    fn sl_28_an_empty_menu_an_expired_offer_and_a_wrong_size_are_refused() {
        assert_eq!(
            choose_offer(&[], 50_000_000, u64::MAX, NOW),
            Err(Refusal::NoOffer)
        );
        let menu = [params(1_000_000, 10_000, 1008)];
        assert_eq!(
            choose_offer(&menu, 50_000_000, u64::MAX, NOW + 7200),
            Err(Refusal::Expired)
        );
        assert_eq!(
            choose_offer(&menu, 999_999, u64::MAX, NOW),
            Err(Refusal::AmountOutsideLimits)
        );
        assert_eq!(
            choose_offer(&menu, 1_000_000_001, u64::MAX, NOW),
            Err(Refusal::AmountOutsideLimits)
        );
    }
}
