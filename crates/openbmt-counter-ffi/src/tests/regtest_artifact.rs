//! Safety gates for the checkout-only native Regtest artifact.
#[cfg(feature = "checkout-regtest")]
mod feature_build {
    use std::sync::atomic::{AtomicU64, Ordering};

    use bitcoin::Network;
    use openbmt_core::batch::BatchPolicy;
    use openbmt_watch::{
        check_blocks_checked, BlockProgress, HealthInput, Peer, PeerAddr, PeerTransport, Problem,
        MAX_BEHIND_SECS, MAX_CLOCK_BEHIND_SECS, MAX_TIP_AGE_SECS,
    };

    use super::super::kit::{record, start_config, Rig};
    use crate::{
        host::{checkout_regtest_health, Entry},
        CounterError,
    };

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn refuses_mainnet_testnet4_testnet_and_signet_before_creating_data_or_starting_a_node() {
        for network in [
            Network::Bitcoin,
            Network::Testnet,
            Network::Testnet4,
            Network::Signet,
        ] {
            let dir = std::env::temp_dir().join(format!(
                "openbmt-checkout-regtest-refusal-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed),
            ));
            assert!(!dir.exists(), "test path already exists: {}", dir.display());
            let mut paired = record();
            paired.network = network;
            let mut req = start_config(dir.to_str().expect("temporary path is UTF-8"));
            req.record = paired.to_bytes();

            let result = Entry::start(
                req,
                Box::new(|_| Err("driver must not start".to_owned())),
                BatchPolicy { min_count: 1 },
            );
            match result {
                Err(CounterError::Refused { rule, .. }) => {
                    // The current record codec does not represent Testnet4;
                    // it is refused while decoding, before the artifact gate.
                    let expected = if network == Network::Testnet4 {
                        "CR-97"
                    } else {
                        "checkout-regtest-network"
                    };
                    assert_eq!(rule, expected);
                }
                Err(other) => panic!("unexpected refusal: {other:?}"),
                Ok(_) => panic!("the checkout Regtest artifact accepted {network:?}"),
            }
            assert!(!dir.exists(), "rejected startup created {}", dir.display());
        }
    }

    #[test]
    fn regtest_driver_allows_local_core_hints_with_a_single_peer_target() {
        let rig = Rig::new();
        let mut paired = record();
        paired.peers = vec!["10.0.2.2:18444".to_owned(), "127.0.0.1:18444".to_owned()];
        let dir = rig.dir();
        let mut req = start_config(dir.to_str().expect("temporary path is UTF-8"));
        req.record = paired.to_bytes();

        let handle = rig.start_with(&req);
        let driver = rig.launch().driver;
        assert!(driver.allow_local);
        assert_eq!(driver.target_peers, 1);
        assert!(driver.seeds.is_empty());
        assert_eq!(
            driver.fixed_peers,
            vec![
                "10.0.2.2:18444".parse().expect("emulator host socket"),
                "127.0.0.1:18444".parse().expect("loopback socket"),
            ]
        );

        let response = rig.call(2, handle, &[]);
        assert!(super::super::wire::Rd::error(&response).is_none());
    }

    fn peer(ip: &str, serves_filters: bool, best_height: u32) -> Peer {
        Peer {
            addr: PeerAddr::Ip(ip.parse().expect("test IP address")),
            serves_filters,
            best_height,
            transport: PeerTransport::V1,
        }
    }

    fn health_input<'a>(
        peers: &'a [Peer],
        tip_height: u32,
        tip_time: u64,
        now: u64,
        behind_since: Option<u64>,
    ) -> HealthInput<'a> {
        HealthInput {
            peers,
            tip_height,
            tip_time,
            now,
            behind_since,
        }
    }

    #[test]
    fn checkout_health_relaxes_only_quorum_and_requires_a_filter_peer() {
        let empty = [];
        let no_peers = checkout_regtest_health(&health_input(&empty, 100, 10_000, 10_000, None));
        assert!(!no_peers.healthy);

        let non_filter = [peer("8.8.8.8", false, 100)];
        let no_filters =
            checkout_regtest_health(&health_input(&non_filter, 100, 10_000, 10_000, None));
        assert!(!no_filters.healthy);
        assert_eq!(no_filters.problem, Some(Problem::TooFewFilterPeers));

        let one_filter = [peer("8.8.8.8", true, 100)];
        let single = checkout_regtest_health(&health_input(&one_filter, 100, 10_000, 10_000, None));
        assert!(single.healthy);
        assert_eq!(single.problem, None);
    }

    #[test]
    fn checkout_health_keeps_tip_freshness_and_clock_rules() {
        let filter = [peer("8.8.8.8", true, 100)];
        let old_tip = checkout_regtest_health(&health_input(
            &filter,
            100,
            20_000 - MAX_TIP_AGE_SECS - 1,
            20_000,
            None,
        ));
        assert!(!old_tip.healthy);
        assert_eq!(old_tip.problem, Some(Problem::TipIsOld));

        let future_tip = checkout_regtest_health(&health_input(
            &filter,
            100,
            20_000 + MAX_CLOCK_BEHIND_SECS + 1,
            20_000,
            None,
        ));
        assert!(!future_tip.healthy);
        assert_eq!(future_tip.problem, Some(Problem::ClockMismatch));
    }

    #[test]
    fn checkout_health_keeps_header_lag_grace_and_block_progress_checks() {
        let behind_peer = [peer("8.8.8.8", true, 103)];
        let within_grace = checkout_regtest_health(&health_input(
            &behind_peer,
            100,
            20_000,
            20_000,
            Some(20_000 - MAX_BEHIND_SECS),
        ));
        assert!(within_grace.healthy);
        assert!(within_grace.behind);

        let timed_out = checkout_regtest_health(&health_input(
            &behind_peer,
            100,
            20_000,
            20_000,
            Some(20_000 - MAX_BEHIND_SECS - 1),
        ));
        assert!(!timed_out.healthy);
        assert!(timed_out.behind);
        assert_eq!(timed_out.problem, Some(Problem::HeadersBehind));

        // This helper does not bypass WR-54: Node applies the block-body
        // frontier after the health callback returns.
        let body_lag = check_blocks_checked(
            within_grace,
            &BlockProgress {
                tip_height: 100,
                checked_height: 99,
                behind_since: Some(20_000 - 16),
                now: 20_000,
            },
        );
        assert!(!body_lag.healthy);
        assert_eq!(body_lag.problem, Some(Problem::BlocksBehind));
    }
}

#[cfg(not(feature = "checkout-regtest"))]
mod default_build {
    use bitcoin::Network;

    use super::super::kit::{record, Rig};
    use crate::tests::kit::start_config;

    #[test]
    fn default_artifact_keeps_existing_non_regtest_start_behavior() {
        let rig = Rig::new();
        let mut paired = record();
        paired.network = Network::Bitcoin;
        let dir = rig.dir();
        let mut req = start_config(dir.to_str().expect("temporary path is UTF-8"));
        req.record = paired.to_bytes();
        let handle = rig.start_with(&req);
        assert_eq!(rig.launch().driver.network, Network::Bitcoin);
        assert!(!rig.launch().driver.allow_local);
        let response = rig.call(2, handle, &[]);
        assert!(super::super::wire::Rd::error(&response).is_none());
    }
}
