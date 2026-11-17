//! A bounded, app-facing snapshot of receiving readiness.
//!
//! The caller supplies live node/backup/chain facts and channel facts copied
//! from LDK. Only usable channels whose funding is known at the configured
//! depth contribute to room. This is an assessment, not a route guarantee:
//! actual routing, fees and channel state can change before payment.

use crate::room::ROOM_MARGIN_MSAT;

/// Maximum age of an assessment. Callers should refresh more often; older
/// snapshots fail closed.
pub const MAX_ASSESSMENT_AGE_SECS: u64 = 30;

/// Minimal channel facts needed for a private receiving-capacity assessment.
/// No channel or funding identifiers are exposed or needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelFact {
    pub usable: bool,
    pub inbound_capacity_msat: u64,
    /// None means unknown or unconfirmed.
    pub funding_confirmations: Option<u32>,
    pub has_funding_output: bool,
}

/// Facts collected for one assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssessmentInput<'a> {
    pub observed_at_unix: u64,
    pub now_unix: u64,
    pub node_running: bool,
    pub backup_reachable: bool,
    pub backup_caught_up: bool,
    pub chain_healthy: bool,
    pub confirmation_depth: u32,
    pub channels: &'a [ChannelFact],
    /// Optional requested payment amount.
    pub amount_msat: Option<u64>,
    /// Opening fee quoted for this amount, when a new channel is needed.
    pub opening_fee_msat: Option<u64>,
    /// Optional caller-supplied fee limit. No policy is inferred when absent.
    pub max_opening_fee_msat: Option<u64>,
}

/// Why the assessment does not say an amount can be received now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotReady {
    NodeStopped,
    BackupUnavailable,
    BackupBehind,
    ChainUnavailable,
    StaleSnapshot,
    NoAmount,
    NoConfirmedCapacity,
    AmountExceedsConfirmedCapacity,
    OpeningFeeUnknown,
    OpeningFeeLimitUnknown,
    OpeningFeeAboveLimit,
}

/// Whether the quoted opening fee is applicable and within policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningFee {
    NotApplicable,
    Unknown,
    WithinLimit { fee_msat: u64 },
    LimitUnknown { fee_msat: u64 },
    AboveLimit { fee_msat: u64, limit_msat: u64 },
}

/// Assessment suitable for an app. Confirmed room is usable inbound capacity
/// after the established margin; it is not a guaranteed route amount. Pending
/// capacity is reported separately and never counted as ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivingAssessment {
    pub observed_at_unix: u64,
    pub confirmed_room_msat: u64,
    pub pending_or_unknown_channels: usize,
    pub opening_fee: OpeningFee,
    /// True only when the snapshot is healthy and the requested amount fits
    /// confirmed room. It does not promise a route or account for capacity
    /// fragmentation across channels.
    pub ready_now: bool,
    pub not_ready: Vec<NotReady>,
}

/// Assess whether receiving is currently plausible for the requested amount.
/// Independent blockers are returned together for clear app explanations.
pub fn assess(input: &AssessmentInput<'_>) -> ReceivingAssessment {
    let depth = input.confirmation_depth.max(1);
    let mut confirmed_sum = 0u64;
    let mut pending_or_unknown_channels = 0usize;
    for channel in input.channels.iter().filter(|c| c.usable) {
        if channel.has_funding_output && channel.funding_confirmations.is_some_and(|n| n >= depth) {
            confirmed_sum = confirmed_sum.saturating_add(channel.inbound_capacity_msat);
        } else {
            pending_or_unknown_channels = pending_or_unknown_channels.saturating_add(1);
        }
    }
    let confirmed_room_msat = confirmed_sum.saturating_sub(ROOM_MARGIN_MSAT);
    let mut not_ready = Vec::new();
    if !input.node_running {
        not_ready.push(NotReady::NodeStopped);
    }
    if !input.backup_reachable {
        not_ready.push(NotReady::BackupUnavailable);
    } else if !input.backup_caught_up {
        not_ready.push(NotReady::BackupBehind);
    }
    if !input.chain_healthy {
        not_ready.push(NotReady::ChainUnavailable);
    }
    if input.now_unix < input.observed_at_unix
        || input.now_unix.saturating_sub(input.observed_at_unix) > MAX_ASSESSMENT_AGE_SECS
    {
        not_ready.push(NotReady::StaleSnapshot);
    }

    let fee_needed = input
        .amount_msat
        .is_some_and(|amount| amount > confirmed_room_msat);
    let opening_fee = if !fee_needed {
        OpeningFee::NotApplicable
    } else {
        match (input.opening_fee_msat, input.max_opening_fee_msat) {
            (None, _) => OpeningFee::Unknown,
            (Some(fee_msat), None) => OpeningFee::LimitUnknown { fee_msat },
            (Some(fee_msat), Some(limit_msat)) if fee_msat <= limit_msat => {
                OpeningFee::WithinLimit { fee_msat }
            }
            (Some(fee_msat), Some(limit_msat)) => OpeningFee::AboveLimit {
                fee_msat,
                limit_msat,
            },
        }
    };

    match input.amount_msat {
        None | Some(0) => not_ready.push(NotReady::NoAmount),
        Some(amount) if confirmed_room_msat == 0 => {
            not_ready.push(NotReady::NoConfirmedCapacity);
            if amount > 0 {
                match opening_fee {
                    OpeningFee::Unknown => not_ready.push(NotReady::OpeningFeeUnknown),
                    OpeningFee::LimitUnknown { .. } => {
                        not_ready.push(NotReady::OpeningFeeLimitUnknown)
                    }
                    OpeningFee::AboveLimit { .. } => not_ready.push(NotReady::OpeningFeeAboveLimit),
                    _ => {}
                }
            }
        }
        Some(amount) if amount > confirmed_room_msat => {
            not_ready.push(NotReady::AmountExceedsConfirmedCapacity);
            match opening_fee {
                OpeningFee::Unknown => not_ready.push(NotReady::OpeningFeeUnknown),
                OpeningFee::LimitUnknown { .. } => not_ready.push(NotReady::OpeningFeeLimitUnknown),
                OpeningFee::AboveLimit { .. } => not_ready.push(NotReady::OpeningFeeAboveLimit),
                _ => {}
            }
        }
        _ => {}
    }
    ReceivingAssessment {
        observed_at_unix: input.observed_at_unix,
        confirmed_room_msat,
        pending_or_unknown_channels,
        opening_fee,
        ready_now: not_ready.is_empty(),
        not_ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(channels: &'a [ChannelFact], amount_msat: Option<u64>) -> AssessmentInput<'a> {
        AssessmentInput {
            observed_at_unix: 100,
            now_unix: 110,
            node_running: true,
            backup_reachable: true,
            backup_caught_up: true,
            chain_healthy: true,
            confirmation_depth: 1,
            channels,
            amount_msat,
            opening_fee_msat: None,
            max_opening_fee_msat: Some(10_000),
        }
    }

    #[test]
    fn only_confirmed_usable_known_fundings_count_toward_room() {
        let channels = [
            ChannelFact {
                usable: true,
                inbound_capacity_msat: 3_000_000,
                funding_confirmations: Some(1),
                has_funding_output: true,
            },
            ChannelFact {
                usable: true,
                inbound_capacity_msat: 9_000_000,
                funding_confirmations: None,
                has_funding_output: true,
            },
            ChannelFact {
                usable: true,
                inbound_capacity_msat: 8_000_000,
                funding_confirmations: Some(10),
                has_funding_output: false,
            },
            ChannelFact {
                usable: false,
                inbound_capacity_msat: 7_000_000,
                funding_confirmations: Some(10),
                has_funding_output: true,
            },
        ];
        let assessed = assess(&input(&channels, Some(1_000_000)));
        assert_eq!(assessed.confirmed_room_msat, 2_000_000);
        assert_eq!(assessed.pending_or_unknown_channels, 2);
        assert!(assessed.ready_now);
    }

    #[test]
    fn configured_depth_and_margin_are_enforced_with_saturating_bounds() {
        let channels = [
            ChannelFact {
                usable: true,
                inbound_capacity_msat: u64::MAX,
                funding_confirmations: Some(1),
                has_funding_output: true,
            },
            ChannelFact {
                usable: true,
                inbound_capacity_msat: 500_000,
                funding_confirmations: Some(1),
                has_funding_output: true,
            },
        ];
        let mut facts = input(&channels, Some(1));
        facts.confirmation_depth = 2;
        let assessed = assess(&facts);
        assert_eq!(assessed.confirmed_room_msat, 0);
        assert_eq!(assessed.pending_or_unknown_channels, 2);
        assert!(assessed.not_ready.contains(&NotReady::NoConfirmedCapacity));
    }

    #[test]
    fn zero_depth_means_one_and_two_confirmed_overflowing_channels_saturate() {
        let channels = [
            ChannelFact {
                usable: true,
                inbound_capacity_msat: u64::MAX,
                funding_confirmations: Some(1),
                has_funding_output: true,
            },
            ChannelFact {
                usable: true,
                inbound_capacity_msat: u64::MAX,
                funding_confirmations: Some(1),
                has_funding_output: true,
            },
        ];
        let mut facts = input(&channels, Some(1));
        facts.confirmation_depth = 0;
        let assessed = assess(&facts);
        assert_eq!(assessed.confirmed_room_msat, u64::MAX - ROOM_MARGIN_MSAT);
        assert_eq!(assessed.pending_or_unknown_channels, 0);
        assert!(assessed.ready_now);
    }

    #[test]
    fn missing_zero_and_margin_sized_capacity_are_not_ready() {
        let channels = [
            ChannelFact {
                usable: true,
                inbound_capacity_msat: 999_000,
                funding_confirmations: Some(1),
                has_funding_output: true,
            },
            ChannelFact {
                usable: true,
                inbound_capacity_msat: 1_000,
                funding_confirmations: Some(1),
                has_funding_output: true,
            },
        ];
        for amount in [None, Some(0), Some(1)] {
            let assessed = assess(&input(&channels, amount));
            assert_eq!(assessed.confirmed_room_msat, 0);
            if amount == Some(1) {
                assert!(assessed.not_ready.contains(&NotReady::NoConfirmedCapacity));
            } else {
                assert!(assessed.not_ready.contains(&NotReady::NoAmount));
            }
            assert!(!assessed.ready_now);
        }
    }

    #[test]
    fn stopped_node_and_unavailable_backup_are_independent_blockers() {
        let channels = [ChannelFact {
            usable: true,
            inbound_capacity_msat: 4_000_000,
            funding_confirmations: Some(1),
            has_funding_output: true,
        }];
        let mut facts = input(&channels, Some(1));
        facts.node_running = false;
        facts.backup_reachable = false;
        facts.backup_caught_up = false;
        let assessed = assess(&facts);
        assert!(assessed.not_ready.contains(&NotReady::NodeStopped));
        assert!(assessed.not_ready.contains(&NotReady::BackupUnavailable));
        assert!(!assessed.not_ready.contains(&NotReady::BackupBehind));
        assert!(!assessed.ready_now);
    }

    #[test]
    fn amount_over_room_reports_fee_state_without_calling_it_ready() {
        let channels = [ChannelFact {
            usable: true,
            inbound_capacity_msat: 5_000_000,
            funding_confirmations: Some(1),
            has_funding_output: true,
        }];
        let mut facts = input(&channels, Some(5_000_000));
        facts.opening_fee_msat = Some(10_001);
        let assessed = assess(&facts);
        assert_eq!(
            assessed.opening_fee,
            OpeningFee::AboveLimit {
                fee_msat: 10_001,
                limit_msat: 10_000
            }
        );
        assert!(assessed
            .not_ready
            .contains(&NotReady::AmountExceedsConfirmedCapacity));
        assert!(assessed.not_ready.contains(&NotReady::OpeningFeeAboveLimit));
        assert!(!assessed.ready_now);
    }

    #[test]
    fn stale_future_dated_and_unhealthy_snapshots_fail_closed() {
        let channels = [ChannelFact {
            usable: true,
            inbound_capacity_msat: 5_000_000,
            funding_confirmations: Some(1),
            has_funding_output: true,
        }];
        let mut facts = input(&channels, Some(1));
        facts.now_unix = 131;
        facts.backup_caught_up = false;
        facts.chain_healthy = false;
        let assessed = assess(&facts);
        assert!(assessed.not_ready.contains(&NotReady::StaleSnapshot));
        assert!(assessed.not_ready.contains(&NotReady::BackupBehind));
        assert!(assessed.not_ready.contains(&NotReady::ChainUnavailable));
        assert!(!assessed.ready_now);
        facts.now_unix = 99;
        assert!(assess(&facts).not_ready.contains(&NotReady::StaleSnapshot));
    }
}
