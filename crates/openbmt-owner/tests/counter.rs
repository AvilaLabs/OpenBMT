//! Adding a counter: its request and four words, the settings and first
//! batch queued with it, the bundle it takes and its hand-over (AS-69 to
//! AS-73).

mod common;

use common::*;
use openbmt_core::bundle::CounterBundle;
use openbmt_owner::{equal_shares, BundleGap, Delivery, OwnerError, Refusal, Stage};

/// $100,000 a bitcoin, which is 1,000 satoshis to a dollar.
const PRICE: u64 = 100_000;

/// Queue counter `n`'s request, its settings and its batch of 3.
fn add(rig: &mut Rig, n: u8) -> openbmt_owner::CounterQueued {
    let (_, request) = pair_request(&rig.owner, n);
    let queued = ok(rig.owner.queue_counter_request(&request, nop));
    ok(rig
        .owner
        .queue_counter(queued.id, PRICE, SMALL_BATCH, NOW, nop))
}

#[test]
fn as_69_a_request_is_queued_with_the_four_words_the_counter_shows() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let (pairing, request) = pair_request(&o, 1);
    let queued = ok(o.queue_counter_request(&request, nop));
    assert_eq!(queued.counter, counter(1).device_id());
    assert_eq!(queued.name, "Counter 1");
    assert_eq!(queued.words, pairing.session_words());
    assert_eq!(queued_pairing_of(&o, queued.id).stage, Stage::Queued);
    let words = queued.words();
    assert!(words.quoted.is_some() && words.code == Some("AS-69"));
}

#[test]
fn as_69_a_code_that_is_not_a_request_and_a_request_for_another_shop_are_refused() {
    let mut o = new_owner(openbmt_owner::Options::default());
    assert!(matches!(
        refused(o.queue_counter_request(b"junk", nop)),
        Refusal::PairingNotRequest(_)
    ));
    // The Key app's own offer is not a request either.
    let offer =
        openbmt_core::pairing::PairOffer::new(o.owner_id(), o.approver_id(), wallet_id(), NET)
            .unwrap();
    assert!(matches!(
        refused(o.queue_counter_request(&offer.to_bytes(), nop)),
        Refusal::PairingNotRequest(_)
    ));
    // Another Key app, and another network, make another offer.
    let other = identity([0x55; 32]).device_id();
    let wrong_approver =
        openbmt_core::pairing::PairOffer::new(o.owner_id(), other, wallet_id(), NET).unwrap();
    let wrong_network = openbmt_core::pairing::PairOffer::new(
        o.owner_id(),
        o.approver_id(),
        wallet_id(),
        bitcoin::Network::Bitcoin,
    )
    .unwrap();
    for offer in [wrong_approver, wrong_network] {
        let (_, request) = openbmt_core::pairing::CounterPairing::begin(
            &offer.to_bytes(),
            &counter(1),
            "Counter 1",
            [1; 32],
            NOW,
        )
        .unwrap();
        assert_eq!(
            refused(o.queue_counter_request(&request.to_bytes(), nop)),
            Refusal::PairingOtherShop
        );
    }
    assert!(o.pairings().is_empty());
}

#[test]
fn as_69_a_counter_that_is_queued_waiting_or_listed_is_refused() {
    let mut rig = Rig::new();
    let (_, request) = pair_request(&rig.owner, 1);
    ok(rig.owner.queue_counter_request(&request, nop));
    // In the queue.
    assert_eq!(
        refused(rig.owner.queue_counter_request(&request, nop)),
        Refusal::PairingKnown
    );
    ok(rig.owner.queue_counter(1, PRICE, SMALL_BATCH, NOW, nop));
    rig.round(&[], 0);
    // Finished and waiting in the outbox, and listed in the settings held.
    assert_eq!(
        refused(rig.owner.queue_counter_request(&request, nop)),
        Refusal::PairingKnown
    );
    ok(rig
        .owner
        .mark_counter_delivered(&counter(1).device_id(), nop));
    assert_eq!(
        refused(rig.owner.queue_counter_request(&request, nop)),
        Refusal::PairingKnown
    );
    assert_eq!(rig.owner.pairings().len(), 0);
}

#[test]
fn as_70_with_no_settings_a_price_is_needed_and_the_first_counter_has_all_of_the_budget() {
    let mut rig = Rig::new();
    assert!(!rig.owner.has_settings_base());
    let (_, request) = pair_request(&rig.owner, 1);
    let queued = ok(rig.owner.queue_counter_request(&request, nop));
    let before = rig.owner.snapshot().unwrap();
    assert_eq!(
        refused(rig.owner.queue_counter(queued.id, 0, SMALL_BATCH, NOW, nop)),
        Refusal::NoSettingsRate
    );
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    // A price outside $1,000 to $10,000,000 is refused and nothing is queued.
    for bad in [1, 999, 10_000_001] {
        assert_eq!(
            refused(
                rig.owner
                    .queue_counter(queued.id, bad, SMALL_BATCH, NOW, nop)
            ),
            Refusal::PriceOutOfRange(openbmt_core::currency::ShopCurrency::Usd)
        );
    }
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    let made = ok(rig
        .owner
        .queue_counter(queued.id, PRICE, SMALL_BATCH, NOW, nop));
    assert_eq!(made.settings_version, 1);
    assert_eq!(made.shares, vec![(counter(1).device_id(), 10_000)]);
    let s = &rig.owner.queued_settings()[0].signed.settings;
    assert!(s.devices.contains(&counter(1).device_id()));
    assert_eq!(s.light_per_sale.display_usd, {
        openbmt_core::settings::Settings::defaults(PRICE)
            .light_per_sale
            .display_usd
    });
    // The starting limits are counted at the price: $100,000 is 1,000 sats a dollar.
    assert_eq!(
        s.light_per_sale.sats,
        u64::from(s.light_per_sale.display_usd) * 1_000
    );
    assert_eq!(made.words().code, Some("AS-70"));
}

#[test]
fn as_70_the_shares_are_split_equally_and_the_earliest_counter_takes_the_rest() {
    let ids = |n: u8| {
        (1..=n)
            .map(|c| counter(c).device_id())
            .collect::<std::collections::BTreeSet<_>>()
    };
    for n in 1..=7u8 {
        let shares = equal_shares(&ids(n));
        let total: u32 = shares.values().map(|s| u32::from(*s)).sum();
        assert_eq!(total, 10_000, "{n} counters");
        let each = 10_000 / u32::from(n);
        let mut it = shares.values();
        assert_eq!(u32::from(*it.next().unwrap()), each + 10_000 % u32::from(n));
        assert!(it.all(|s| u32::from(*s) == each));
    }
    assert!(equal_shares(&std::collections::BTreeSet::new()).is_empty());

    // Through the queue: each counter is added to the settings that were in
    // force, and the version goes up by one each time.
    let mut rig = Rig::new();
    add(&mut rig, 1);
    rig.round(&[], 0);
    let second = add(&mut rig, 2);
    assert_eq!(second.settings_version, 2);
    assert!(second.shares.iter().all(|(_, s)| *s == 5_000));
    rig.round(&[], 0);
    let third = add(&mut rig, 3);
    assert_eq!(third.settings_version, 3);
    let mut shares: Vec<u16> = third.shares.iter().map(|(_, s)| *s).collect();
    shares.sort_unstable();
    assert_eq!(shares, vec![3_333, 3_333, 3_334]);
    assert_eq!(
        third.shares.first().map(|(_, s)| *s),
        Some(3_334),
        "the earliest key takes the rest"
    );
}

#[test]
fn as_70_a_counter_added_before_the_last_is_approved_stays_in_the_new_settings() {
    let mut rig = Rig::new();
    add(&mut rig, 1);
    let second = add(&mut rig, 2);
    // The first counter's queued settings were superseded by the second's,
    // which list both.
    assert_eq!(rig.owner.queued_settings().len(), 1);
    assert_eq!(second.shares.len(), 2);
    let s = &rig.owner.queued_settings()[0].signed.settings;
    assert!(s.counters.contains_key(&counter(1).device_id()));
    assert!(s.counters.contains_key(&counter(2).device_id()));
    assert_eq!(s.version, 2);
    // One session approves both counters.
    let taken = rig.round(&[], 0);
    assert!(taken.outcomes.len() >= 5);
    assert_eq!(rig.owner.counters_waiting().len(), 2);
}

#[test]
fn as_71_the_batch_starts_at_the_first_free_index_and_is_queued_with_the_settings() {
    let mut rig = Rig::new();
    let first = add(&mut rig, 1);
    let second = add(&mut rig, 2);
    let b = rig.owner.batches();
    assert_eq!(b.len(), 2);
    assert_eq!((b[0].batch.start, b[0].batch.count), (0, SMALL_BATCH));
    assert_eq!(
        (b[1].batch.start, b[1].batch.count),
        (SMALL_BATCH, SMALL_BATCH)
    );
    assert_eq!(b[0].id, first.batch_id);
    assert_eq!(b[1].id, second.batch_id);
    assert_eq!(rig.owner.receive_mark(), 2 * SMALL_BATCH);
}

#[test]
fn as_71_nothing_is_queued_unless_the_pairing_the_settings_and_the_batch_all_are() {
    let mut rig = Rig::new();
    let (_, request) = pair_request(&rig.owner, 1);
    let queued = ok(rig.owner.queue_counter_request(&request, nop));
    let before = rig.owner.snapshot().unwrap();
    // A batch below the owner phone's own minimum is not derived.
    assert_eq!(
        refused(rig.owner.queue_counter(queued.id, PRICE, 2, NOW, nop)),
        Refusal::BatchNotDerived
    );
    // A pairing that is not queued has no counter to add.
    assert_eq!(
        refused(rig.owner.queue_counter(99, PRICE, SMALL_BATCH, NOW, nop)),
        Refusal::UnknownEntry(99)
    );
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    // The batch queue is full: the settings are not queued either.
    for i in 0..16 {
        ok(rig.owner.queue_batch(batch(9, i * 3, 3), 1_000, nop));
    }
    let before = rig.owner.snapshot().unwrap();
    assert!(matches!(
        refused(
            rig.owner
                .queue_counter(queued.id, PRICE, SMALL_BATCH, NOW, nop)
        ),
        Refusal::Full("the batch queue")
    ));
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert!(rig.owner.queued_settings().is_empty());
}

#[test]
fn as_71_a_store_that_fails_changes_nothing() {
    let mut rig = Rig::new();
    let (_, request) = pair_request(&rig.owner, 1);
    let queued = ok(rig.owner.queue_counter_request(&request, nop));
    let before = rig.owner.snapshot().unwrap();
    let err = rig
        .owner
        .queue_counter(queued.id, PRICE, SMALL_BATCH, NOW, |_| Err::<(), _>("disk"))
        .expect_err("refused");
    assert!(matches!(err, OwnerError::Store("disk")));
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert_eq!(rig.owner.receive_mark(), 0);
}

#[test]
fn as_72_the_bundle_is_made_from_what_the_key_app_approved() {
    let mut rig = Rig::new();
    let made = add(&mut rig, 1);
    let c = counter(1).device_id();
    // Before the approval there is nothing to show.
    assert_eq!(
        rig.owner.counter_bundle(&c).unwrap_err(),
        Refusal::BundleNotReady(BundleGap::Pairing)
    );
    rig.round(&[], 0);
    let bundle = rig.owner.counter_bundle(&c).expect("the bundle");
    assert_eq!(bundle.bundle.pairing.entry.counter, c);
    assert_eq!(
        bundle.bundle.settings.settings.version,
        made.settings_version
    );
    assert!(bundle.bundle.settings.approver_sig.is_some());
    let first = bundle.bundle.first_batch.as_ref().expect("the first batch");
    assert_eq!((first.batch.counter, first.batch.start), (c, 0));
    assert!(bundle.bundle.peers.is_empty());
    assert!(bundle.parts.batch.is_some());
    // The bytes read back as they are, and nothing was changed.
    let bytes = bundle.bundle.to_bytes();
    assert_eq!(CounterBundle::from_bytes(&bytes).unwrap(), bundle.bundle);
    assert_eq!(rig.owner.outbox().len(), 3);
    assert_eq!(bundle.words().code, Some("AS-72"));
    // Another counter has no pairing here.
    assert_eq!(
        rig.owner
            .counter_bundle(&counter(2).device_id())
            .unwrap_err(),
        Refusal::BundleNotReady(BundleGap::Pairing)
    );
}

#[test]
fn as_72_a_counter_with_no_batch_waiting_gets_a_bundle_without_one_and_says_so() {
    let mut rig = Rig::new();
    let (_, request) = pair_request(&rig.owner, 1);
    ok(rig.owner.queue_counter_request(&request, nop));
    let mut s = settings_with(1, &[1]);
    s.version = 1;
    ok(rig.owner.queue_settings(s, nop));
    rig.round(&[], 0);
    let bundle = rig
        .owner
        .counter_bundle(&counter(1).device_id())
        .expect("a bundle");
    assert!(bundle.bundle.first_batch.is_none() && bundle.parts.batch.is_none());
    assert_eq!(bundle.words().heading, "Code has no addresses");
}

#[test]
fn as_72_settings_that_do_not_list_the_counter_are_a_gap() {
    let mut rig = Rig::new();
    let (_, request) = pair_request(&rig.owner, 1);
    ok(rig.owner.queue_counter_request(&request, nop));
    // The session carries settings for another counter.
    ok(rig.owner.queue_settings(settings_with(1, &[2]), nop));
    rig.round(&[], 0);
    assert_eq!(
        rig.owner
            .counter_bundle(&counter(1).device_id())
            .unwrap_err(),
        Refusal::BundleNotReady(BundleGap::Settings)
    );
}

#[test]
fn as_73_a_waiting_counter_is_listed_until_it_has_taken_its_code() {
    let mut rig = Rig::new();
    add(&mut rig, 1);
    assert!(rig.owner.counters_waiting().is_empty());
    rig.round(&[], 0);
    let waiting = rig.owner.counters_waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].counter, counter(1).device_id());
    assert_eq!(waiting[0].name, "Counter 1");
    // Leaving keeps everything waiting; "show again" gives the same bundle.
    let again = rig.owner.counter_bundle(&counter(1).device_id()).unwrap();
    assert_eq!(
        again,
        rig.owner.counter_bundle(&counter(1).device_id()).unwrap()
    );
    ok(rig
        .owner
        .mark_counter_delivered(&counter(1).device_id(), nop));
    assert!(rig.owner.counters_waiting().is_empty());
    // It was the only counter in its settings: nothing is left waiting.
    assert!(rig.owner.outbox().is_empty());
    // Delivered twice has nothing to deliver.
    assert_eq!(
        refused(
            rig.owner
                .mark_counter_delivered(&counter(1).device_id(), nop)
        ),
        Refusal::BundleNotReady(BundleGap::Pairing)
    );
}

#[test]
fn as_73_the_settings_wait_on_for_the_counters_that_have_not_had_them() {
    let mut rig = Rig::new();
    add(&mut rig, 1);
    rig.round(&[], 0);
    ok(rig
        .owner
        .mark_counter_delivered(&counter(1).device_id(), nop));
    add(&mut rig, 2);
    rig.round(&[], 0);
    ok(rig
        .owner
        .mark_counter_delivered(&counter(2).device_id(), nop));
    // The settings of version 2 list counter 1 too, which has not had them.
    let left: Vec<_> = rig.owner.outbox().iter().map(|d| &d.what).collect();
    assert_eq!(left.len(), 1);
    assert!(matches!(left[0], Delivery::Settings(_)));
}
