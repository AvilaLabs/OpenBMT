//! The cap and the sweep (SL-33, SL-34): once the node's balance passes the
//! cap, everything above half the cap goes to the shop's wallet, in one splice
//! out. The cap is the owner's; the node only does the arithmetic and the
//! splice.

/// What to move out: nothing while the balance is at or under the cap, and
/// above it everything over half the cap (SL-33).
pub fn sweep_amount_sat(balance_sat: u64, cap_sat: u64) -> Option<u64> {
    if balance_sat <= cap_sat {
        return None;
    }
    Some(balance_sat - cap_sat / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sl_33_nothing_moves_at_or_under_the_cap() {
        assert_eq!(sweep_amount_sat(0, 100_000), None);
        assert_eq!(sweep_amount_sat(100_000, 100_000), None);
    }

    #[test]
    fn sl_33_above_the_cap_everything_above_half_the_cap_moves() {
        assert_eq!(sweep_amount_sat(100_001, 100_000), Some(50_001));
        assert_eq!(sweep_amount_sat(300_000, 100_000), Some(250_000));
        assert_eq!(sweep_amount_sat(1, 0), Some(1));
    }
}
