//! The owner phone of a shop with its own wallet (`docs/OWNER-APP-RULES.md`
//! section 17, OA-105 to OA-114): the shop's state, the allocation book, the
//! offer, the counter's request, the bundle and the counts. The counter's
//! side is the core's `OwnCounterPairing`; the counter node is in the
//! counter crates' tests and the end-to-end test of the counter FFI.

use std::cell::RefCell;
use std::collections::BTreeSet;

use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::{OwnCounterPairing, OwnRequest};
use openbmt_core::pinned_wallet::{PinnedWallet, SignedRange};
use openbmt_core::say::check;
use openbmt_owner::own_book::{
    gap_limit, gap_needs_notice, needs_range, unused_indexes, AllocationBook, BookError,
    MAX_DEVICES, RANGE_LEN, TOP_UP_BELOW, WALLET_GAP,
};
use openbmt_owner::own_say::{all_own_words, is_placeholder};
use openbmt_owner::own_shop::{
    MadeOffer, OwnError, OwnRefusal, OwnRestoreError, OwnShop, MAX_REQUEST_BYTES, OFFER_WINDOW_SECS,
};

const SINGLE: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
const MULTI: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";
const T0: u64 = 1_000_000;
const USD: u64 = 100_000;

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

fn shop(text: &str) -> OwnShop {
    OwnShop::new(id(1), PinnedWallet::from_text(text).unwrap())
}

/// A store that keeps the last bytes written.
struct Disk(RefCell<Vec<u8>>);

impl Disk {
    fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
    fn put(&self, bytes: &[u8]) -> Result<(), String> {
        *self.0.borrow_mut() = bytes.to_vec();
        Ok(())
    }
    fn get(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }
}

/// A counter that read `made` and made its request.
fn counter_request(made: &MadeOffer, n: u8, name: &str) -> (OwnCounterPairing, OwnRequest) {
    OwnCounterPairing::begin(made.offer.clone(), &id(n), name, [n; 32], made.made_at + 5).unwrap()
}

fn refused<T: std::fmt::Debug>(r: Result<T, OwnError<String>>) -> OwnRefusal {
    match r {
        Err(OwnError::Refused(e)) => e,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn confirm(
    shop: &mut OwnShop,
    disk: &Disk,
    made: &MadeOffer,
    request: &OwnRequest,
    now: u64,
) -> Result<openbmt_owner::own_shop::Paired, OwnError<String>> {
    shop.confirm(made, now, &request.to_bytes(), USD, Vec::new(), None, |b| {
        disk.put(b)
    })
}

#[test]
fn oa_105_the_state_is_stored_before_it_takes_effect_and_reads_back_exactly() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let failing = s.allocate_desk(T0, |_: &[u8]| Err("disk full".to_string()));
    assert!(matches!(failing, Err(OwnError::Store(_))));
    assert_eq!(s.book().ranges_given(), 0, "a failed store changes nothing");
    s.allocate_desk(T0, |b| disk.put(b)).unwrap();
    let back =
        OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), &disk.get()).unwrap();
    assert_eq!(back.to_bytes(), disk.get());
    assert_eq!(back.book(), s.book());
}

#[test]
fn oa_105_a_state_of_another_wallet_or_owner_is_refused_and_every_cut_is_refused() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    s.allocate_desk(T0, |b| disk.put(b)).unwrap();
    let bytes = disk.get();
    let other_wallet = PinnedWallet::from_text(MULTI).unwrap();
    assert_eq!(
        OwnShop::restore(id(1), other_wallet, &bytes).unwrap_err(),
        OwnRestoreError::NotThisShop
    );
    assert_eq!(
        OwnShop::restore(id(2), PinnedWallet::from_text(SINGLE).unwrap(), &bytes).unwrap_err(),
        OwnRestoreError::NotThisShop
    );
    for n in 0..bytes.len() {
        assert!(
            OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), &bytes[..n]).is_err(),
            "cut at {n}"
        );
    }
    let mut longer = bytes;
    longer.push(0);
    assert!(OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), &longer).is_err());
}

#[test]
fn oa_106_a_range_holds_a_thousand_indexes_and_starts_where_the_highest_ended() {
    let mut b = AllocationBook::new();
    let (a, c) = (id(3).device_id(), id(4).device_id());
    let r0 = b.allocate(a, T0).unwrap();
    let r1 = b.allocate(c, T0).unwrap();
    let r2 = b.allocate(a, T0).unwrap();
    assert_eq!((r0.start, r0.end), (0, 1_000));
    assert_eq!((r1.start, r1.end), (1_000, 2_000));
    assert_eq!((r2.start, r2.end), (2_000, 3_000));
    assert_eq!(RANGE_LEN, 1_000);
    assert_eq!(b.ranges_of(&a), vec![r0, r2], "in the order given");
    assert_eq!(b.ranges_of(&c), vec![r1]);
    assert_eq!(b.next_start(), 3_000);
}

#[test]
fn oa_106_no_index_is_given_twice_also_for_the_desk_and_after_a_counter_is_forgotten() {
    let mut s = shop(SINGLE);
    let desk = s.owner_id();
    let counter = id(3).device_id();
    let a = s.allocate(counter, T0, |_| Ok::<(), String>(())).unwrap();
    let d = s.allocate_desk(T0, |_| Ok::<(), String>(())).unwrap();
    assert_eq!(a.range.end, d.range.start, "the desk takes the next range");
    assert_eq!(d.range.counter, desk);
    d.verify(&desk).unwrap();
    // Forgetting a counter removes nothing from the book: the next range
    // still starts at the end of the highest one.
    let c = s
        .allocate(id(4).device_id(), T0, |_| Ok::<(), String>(()))
        .unwrap();
    assert_eq!(c.range.start, d.range.end);
    let mut seen = BTreeSet::new();
    for dev in s.book().devices() {
        for r in s.book().ranges_of(dev) {
            for i in r.start..r.end {
                assert!(seen.insert(i), "index {i} given twice");
            }
        }
    }
    assert_eq!(seen.len(), 3_000);
}

#[test]
fn oa_106_the_range_is_stored_before_it_is_signed_and_signed_before_it_is_returned() {
    let mut s = shop(SINGLE);
    let seen_before_return = RefCell::new(0u32);
    let counter = id(3).device_id();
    let signed = s
        .allocate(counter, T0, |bytes: &[u8]| {
            // At the time of the store the book already holds the range.
            let back = OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), bytes)
                .map_err(|e| e.to_string())?;
            *seen_before_return.borrow_mut() = back.book().ranges_given();
            Ok::<(), String>(())
        })
        .unwrap();
    assert_eq!(*seen_before_return.borrow(), 1);
    signed.verify(&s.owner_id()).unwrap();
    assert_eq!(signed.range.counter, counter);
}

#[test]
fn oa_106_the_book_holds_at_most_sixty_four_devices() {
    let mut b = AllocationBook::new();
    for n in 0..MAX_DEVICES {
        let who = id(u8::try_from(n + 3).unwrap()).device_id();
        b.allocate(who, T0).unwrap();
    }
    assert_eq!(
        b.allocate(id(200).device_id(), T0),
        Err(BookError::TooManyDevices)
    );
}

#[test]
fn oa_106_the_book_decodes_only_what_it_encodes() {
    let mut b = AllocationBook::new();
    for n in [3u8, 4, 3, 5] {
        b.allocate(id(n).device_id(), T0).unwrap();
    }
    let bytes = b.to_bytes();
    assert_eq!(AllocationBook::from_bytes(&bytes).unwrap(), b);
    // One device with two ranges: the second start is 12 bytes from the end.
    let mut two = AllocationBook::new();
    let d = id(3).device_id();
    two.allocate(d, T0).unwrap();
    two.allocate(d, T0).unwrap();
    let good = two.to_bytes();
    assert_eq!(AllocationBook::from_bytes(&good).unwrap(), two);
    for bad_start in [2_000u32, 1_001, 0] {
        let mut bad = good.clone();
        let n = bad.len();
        bad[n - 12..n - 8].copy_from_slice(&bad_start.to_le_bytes());
        assert!(AllocationBook::from_bytes(&bad).is_err(), "{bad_start}");
    }
    let mut bad_magic = bytes;
    bad_magic[3] = b'1';
    assert!(AllocationBook::from_bytes(&bad_magic).is_err());
}

#[test]
fn oa_107_every_index_of_every_range_is_watched_from_the_moment_it_is_written() {
    let mut s = shop(SINGLE);
    let counter = id(3).device_id();
    let seen_in_store = RefCell::new(0..0u32);
    s.allocate(counter, T0, |bytes: &[u8]| {
        let back = OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), bytes)
            .map_err(|e| e.to_string())?;
        *seen_in_store.borrow_mut() = back.book().indexes();
        Ok::<(), String>(())
    })
    .unwrap();
    assert_eq!(*seen_in_store.borrow(), 0..1_000);
    s.allocate_desk(T0, |_| Ok::<(), String>(())).unwrap();
    assert_eq!(s.book().indexes(), 0..2_000);
    assert_eq!(s.book().device_of(0), Some(counter));
    assert_eq!(s.book().device_of(1_999), Some(s.owner_id()));
    assert_eq!(s.book().device_of(2_000), None);
}

#[test]
fn oa_108_the_offer_names_the_owner_the_wallet_and_the_version_in_force() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    assert_eq!(made.offer.floor, 0, "no settings, no floor");
    assert_eq!(made.offer.owner, s.owner_id());
    assert_eq!(made.offer.wallet, PinnedWallet::from_text(SINGLE).unwrap());
    assert_eq!(made.ends_at(), T0 + 600);
    assert_eq!(OFFER_WINDOW_SECS, 600);
    let (_, request) = counter_request(&made, 3, "Front");
    let paired = confirm(&mut s, &disk, &made, &request, T0 + 30).unwrap();
    let next = s.make_offer(T0 + 100).unwrap();
    assert_eq!(next.offer.floor, paired.settings_version);
    // The bytes are the frame the counter reads.
    assert_eq!(next.offer.to_bytes().get(..4), Some(&b"OBM-"[..]));
}

#[test]
fn oa_108_after_ten_minutes_the_offer_has_ended() {
    let s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (_, request) = counter_request(&made, 3, "Front");
    assert!(s.read_request(&made, T0 + 600, &request.to_bytes()).is_ok());
    assert_eq!(
        refused::<()>(
            s.read_request(&made, T0 + 601, &request.to_bytes())
                .map(|_| ())
                .map_err(OwnError::Refused)
        ),
        OwnRefusal::OfferEnded
    );
    assert_eq!(
        s.read_request(&made, T0 - 1, &request.to_bytes())
            .unwrap_err(),
        OwnRefusal::ClockWentBack
    );
    // An offer of another shop is not this shop's.
    let other = shop(MULTI).make_offer(T0).unwrap();
    assert_eq!(
        s.read_request(&other, T0, &request.to_bytes()).unwrap_err(),
        OwnRefusal::NotThisShopsOffer
    );
}

#[test]
fn oa_109_a_request_that_reads_gives_the_name_and_the_same_four_words_as_the_counter() {
    let s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (counter, request) = counter_request(&made, 3, "Front counter");
    let read = s.read_request(&made, T0 + 10, &request.to_bytes()).unwrap();
    assert_eq!(read.name, "Front counter");
    assert_eq!(read.counter, counter.counter());
    assert_eq!(read.words, counter.session_words());
}

#[test]
fn oa_109_each_refusal_has_its_own_words() {
    let s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (_, request) = counter_request(&made, 3, "Front");
    let good = request.to_bytes();
    let read = |bytes: &[u8]| s.read_request(&made, T0 + 10, bytes).unwrap_err();

    // Not a request: another frame, empty, and over 1,024 bytes.
    assert_eq!(read(&[]), OwnRefusal::NotARequest);
    assert_eq!(read(&made.offer.to_bytes()), OwnRefusal::NotARequest);
    let mut long = good.clone();
    long.resize(MAX_REQUEST_BYTES + 1, 0);
    assert_eq!(read(&long), OwnRefusal::NotARequest);

    // For another offer.
    let other_made = MadeOffer {
        offer: shop(MULTI).make_offer(T0).unwrap().offer,
        made_at: T0,
    };
    let (_, other_request) = counter_request(&other_made, 4, "Back");
    assert_eq!(read(&other_request.to_bytes()), OwnRefusal::OtherOffer);

    // A signature that fails.
    let mut forged = request.clone();
    forged.nonce = [9; 32];
    assert_eq!(read(&forged.to_bytes()), OwnRefusal::BadSignature);

    // The owner's own key.
    let (_, from_owner) =
        OwnCounterPairing::begin(made.offer.clone(), &id(7), "Front", [7; 32], T0 + 5).unwrap();
    let mut own = from_owner;
    own.counter = s.owner_id();
    assert_eq!(read(&own.to_bytes()), OwnRefusal::OwnKey);

    // A name that is not plain text: the byte after the name's length.
    let mut bad_name = good;
    let at = 4 + 1 + 32 + 1;
    bad_name[at] = 0x07;
    assert_eq!(read(&bad_name), OwnRefusal::BadName);

    for r in [
        OwnRefusal::OfferEnded,
        OwnRefusal::NotARequest,
        OwnRefusal::OtherOffer,
        OwnRefusal::BadSignature,
        OwnRefusal::OwnKey,
    ] {
        assert!(!is_placeholder(&r.words()), "{r:?}");
    }
}

#[test]
fn oa_110_they_match_makes_the_settings_the_range_and_the_pairing_the_counter_takes() {
    for text in [SINGLE, MULTI] {
        let disk = Disk::new();
        let mut s = shop(text);
        let made = s.make_offer(T0).unwrap();
        let (counter, request) = counter_request(&made, 3, "Front");
        let paired = confirm(&mut s, &disk, &made, &request, T0 + 30).unwrap();
        let bundle = &paired.bundle;
        // The first settings: the owner's signature only, a version above
        // the floor, this counter on the list and holding all the shares.
        assert!(bundle.settings.approver_sig.is_none());
        assert!(bundle.settings.settings.version > made.offer.floor);
        assert!(bundle
            .settings
            .settings
            .devices
            .contains(&counter.counter()));
        assert_eq!(
            bundle.settings.settings.counters.get(&counter.counter()),
            Some(&10_000)
        );
        // The first range: the book's first, signed, 1,000 indexes.
        assert_eq!(
            (paired.first_range.start, paired.first_range.end),
            (0, 1_000)
        );
        bundle.first_range.verify(&s.owner_id()).unwrap();
        assert_eq!(bundle.first_range.range, paired.first_range);
        // The counter takes it.
        counter
            .finish(&bundle.pairing, true, T0 + 40)
            .expect("the counter takes the pairing");
        // Stored: the book and the waiting bundle.
        let back =
            OwnShop::restore(id(1), PinnedWallet::from_text(text).unwrap(), &disk.get()).unwrap();
        assert_eq!(back.counters_waiting().len(), 1);
        assert_eq!(back.book().ranges_given(), 1);
    }
}

#[test]
fn oa_110_a_second_counter_is_listed_with_equal_shares_and_a_higher_version() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (_, r1) = counter_request(&made, 3, "Front");
    let first = confirm(&mut s, &disk, &made, &r1, T0 + 30).unwrap();
    let made2 = s.make_offer(T0 + 100).unwrap();
    let (_, r2) = counter_request(&made2, 4, "Back");
    let second = confirm(&mut s, &disk, &made2, &r2, T0 + 130).unwrap();
    assert!(second.settings_version > first.settings_version);
    assert_eq!(second.shares.len(), 2);
    assert_eq!(
        second
            .shares
            .iter()
            .map(|(_, b)| u32::from(*b))
            .sum::<u32>(),
        10_000
    );
    // The second counter's first range starts where the first one ended.
    assert_eq!(second.first_range.start, 1_000);
}

#[test]
fn oa_110_a_phone_with_no_settings_needs_a_price_and_allocates_nothing_without_one() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (_, request) = counter_request(&made, 3, "Front");
    let r = s.confirm(
        &made,
        T0 + 10,
        &request.to_bytes(),
        0,
        Vec::new(),
        None,
        |b| disk.put(b),
    );
    assert!(matches!(
        refused(r),
        OwnRefusal::Settings(openbmt_owner::Refusal::NoSettingsRate)
    ));
    assert_eq!(s.book().ranges_given(), 0);
    assert!(disk.get().is_empty(), "nothing was stored");
    assert!(!s.has_settings_base());
}

#[test]
fn oa_110_they_differ_and_a_late_or_bad_request_allocate_nothing() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (_, request) = counter_request(&made, 3, "Front");
    // Late.
    assert_eq!(
        refused(confirm(&mut s, &disk, &made, &request, T0 + 601)),
        OwnRefusal::OfferEnded
    );
    // Garbled.
    let r = s.confirm(&made, T0 + 10, &[1, 2, 3], USD, Vec::new(), None, |b| {
        disk.put(b)
    });
    assert_eq!(refused(r), OwnRefusal::NotARequest);
    assert_eq!(s.book().ranges_given(), 0);
    assert!(disk.get().is_empty());
    // "They differ" is the app's: the library holds nothing of the request,
    // so there is nothing to forget, and a later offer still works.
    assert!(confirm(&mut s, &disk, &made, &request, T0 + 20).is_ok());
}

#[test]
fn oa_110_a_counter_added_already_is_refused_and_allocates_nothing_more() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (_, request) = counter_request(&made, 3, "Front");
    confirm(&mut s, &disk, &made, &request, T0 + 10).unwrap();
    assert_eq!(
        refused(confirm(&mut s, &disk, &made, &request, T0 + 20)),
        OwnRefusal::AlreadyAdded
    );
    assert_eq!(s.book().ranges_given(), 1);
}

#[test]
fn oa_111_the_bundle_is_shown_again_unchanged_until_the_owner_says_paired() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (counter, request) = counter_request(&made, 3, "Front");
    let paired = confirm(&mut s, &disk, &made, &request, T0 + 10).unwrap();
    let who = counter.counter();
    assert_eq!(s.counters_waiting().len(), 1);
    let again = s.bundle_again(&who).unwrap();
    assert_eq!(again.to_bytes(), paired.bundle.to_bytes());
    assert_eq!(
        s.book().ranges_given(),
        1,
        "showing again allocates nothing"
    );
    // The stored state shows the same bundle after a restart.
    let back =
        OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), &disk.get()).unwrap();
    assert_eq!(
        back.bundle_again(&who).unwrap().to_bytes(),
        paired.bundle.to_bytes()
    );
    // Only the owner's word marks it paired.
    s.mark_delivered(&who, |b| disk.put(b)).unwrap();
    assert!(s.counters_waiting().is_empty());
    assert_eq!(s.bundle_again(&who).unwrap_err(), OwnRefusal::NoBundle);
    assert_eq!(
        refused(s.mark_delivered(&who, |b| disk.put(b))),
        OwnRefusal::NoBundle
    );
    assert_eq!(s.book().ranges_given(), 1);
}

#[test]
fn oa_112_the_unused_indexes_are_the_held_ones_that_no_sale_opened() {
    let opened: BTreeSet<u32> = (0..750).collect();
    assert_eq!(unused_indexes(&[(0, 1_000)], &opened), 250);
    // A second held range adds its indexes; an opened index outside the
    // held ranges counts for nothing.
    assert_eq!(
        unused_indexes(&[(0, 1_000), (1_000, 2_000)], &opened),
        1_250
    );
    let stray: BTreeSet<u32> = [5_000].into_iter().collect();
    assert_eq!(unused_indexes(&[(0, 1_000)], &stray), 1_000);
    assert_eq!(unused_indexes(&[], &opened), 0);
}

#[test]
fn oa_112_a_counter_below_three_hundred_with_no_range_waiting_is_given_the_next() {
    assert_eq!(TOP_UP_BELOW, 300);
    assert!(needs_range(299, false));
    assert!(!needs_range(300, false));
    assert!(
        !needs_range(0, true),
        "at most one range waits for a counter"
    );
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer(T0).unwrap();
    let (counter, request) = counter_request(&made, 3, "Front");
    confirm(&mut s, &disk, &made, &request, T0 + 10).unwrap();
    let top_up = s
        .allocate_top_up(counter.counter(), T0, |b| disk.put(b))
        .unwrap();
    assert_eq!((top_up.range.start, top_up.range.end), (1_000, 2_000));
    top_up.verify(&s.owner_id()).unwrap();
    // A counter the book does not know gets none.
    let stranger = id(9).device_id();
    assert_eq!(
        refused(s.allocate_top_up(stranger, T0, |b| disk.put(b))),
        OwnRefusal::UnknownCounter
    );
    let _: SignedRange = top_up;
}

#[test]
fn oa_113_the_gap_limit_is_the_longest_run_without_a_payment_plus_one() {
    let paid = |v: &[u32]| v.iter().copied().collect::<BTreeSet<u32>>();
    // Nothing given out, nothing needed.
    assert_eq!(gap_limit(&paid(&[]), None), 0);
    // Index 0 given out and unpaid: a run of 1.
    assert_eq!(gap_limit(&paid(&[]), Some(0)), 2);
    // Paid at once: the wallet needs to look one ahead.
    assert_eq!(gap_limit(&paid(&[0]), Some(0)), 1);
    // A run of 4 unpaid before index 5.
    assert_eq!(gap_limit(&paid(&[0, 5]), Some(5)), 5);
    // A run of 2 at the end, counted to the highest given out.
    assert_eq!(gap_limit(&paid(&[0, 1, 2]), Some(4)), 3);
    // A second counter's first payment at 1,000 with nothing before it.
    assert_eq!(gap_limit(&paid(&[1_000]), Some(1_000)), 1_001);
    // It changes as payments arrive.
    assert_eq!(gap_limit(&paid(&[1_000, 0]), Some(1_000)), 1_000);
}

#[test]
fn oa_113_the_wallets_page_says_so_above_twenty() {
    assert_eq!(WALLET_GAP, 20);
    assert!(!gap_needs_notice(20));
    assert!(gap_needs_notice(21));
}

#[test]
fn oa_114_every_state_and_refusal_has_words_or_the_placeholder_that_names_its_rule() {
    let all = all_own_words();
    assert!(all.len() >= 18);
    for (name, w) in &all {
        if is_placeholder(w) {
            assert!(
                w.heading.starts_with("WORDS_NEEDED_OA_")
                    || w.heading.starts_with("WORDS_NEEDED_SS"),
                "{name}"
            );
        } else {
            assert_eq!(check(w), Ok(()), "{name}");
        }
    }
}

#[test]
fn cr_256_the_offer_of_a_starter_wallet_carries_the_flag_and_another_does_not() {
    let s = shop(SINGLE);
    let plain = s.make_offer(T0).unwrap();
    assert!(!plain.offer.starter);
    assert_eq!(plain.offer.to_bytes()[4], 1, "version 1");
    let starter = s.make_offer_as(T0, true).unwrap();
    assert!(starter.offer.starter);
    assert_eq!(starter.offer.to_bytes()[4], 2, "version 2");
    // The counter that takes it knows the pairing came from a starter wallet.
    let (counter, request) = counter_request(&starter, 3, "Front");
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let paired = confirm(&mut s, &disk, &starter, &request, T0 + 30).unwrap();
    let kept = counter
        .finish(&paired.bundle.pairing, true, T0 + 40)
        .unwrap();
    assert!(kept.offer.starter);
}

#[test]
fn cr_257_the_switch_names_the_new_wallet_and_a_range_above_every_range_given() {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer_as(T0, true).unwrap();
    let (_, request) = counter_request(&made, 3, "Front");
    let counter = request.counter;
    confirm(&mut s, &disk, &made, &request, T0 + 30).unwrap();
    let new = PinnedWallet::from_text(MULTI).unwrap();
    // A failed store changes nothing and makes no message.
    let failing = s.make_switch(counter, &new, T0 + 60, |_: &[u8]| {
        Err("disk full".to_string())
    });
    assert!(matches!(failing, Err(OwnError::Store(_))));
    assert_eq!(s.book().ranges_given(), 1);
    let sw = s
        .make_switch(counter, &new, T0 + 60, |b| disk.put(b))
        .unwrap();
    assert_eq!(sw.counter, counter);
    assert_eq!(sw.from, s.wallet().checksum32());
    assert_eq!(sw.wallet, new);
    // The range comes after the first range and is the book's.
    assert_eq!(
        (sw.first_range.range.start, sw.first_range.range.end),
        (1_000, 2_000)
    );
    assert_eq!(sw.first_range.range.counter, counter);
    assert!(sw.verify(&s.owner_id()));
    assert_eq!(s.book().ranges_given(), 2);
    let back =
        OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), &disk.get()).unwrap();
    assert_eq!(back.book(), s.book());
    // The same wallet, and a counter the book does not know, are refused with
    // nothing allocated.
    let same = PinnedWallet::from_text(SINGLE).unwrap();
    assert_eq!(
        refused(s.make_switch(counter, &same, T0 + 70, |b| disk.put(b))),
        OwnRefusal::SwitchSameWallet
    );
    assert_eq!(
        refused(s.make_switch(id(9).device_id(), &new, T0 + 70, |b| disk.put(b))),
        OwnRefusal::UnknownCounter
    );
    assert_eq!(
        refused(s.make_switch(s.owner_id(), &new, T0 + 70, |b| disk.put(b))),
        OwnRefusal::UnknownCounter
    );
    assert_eq!(s.book().ranges_given(), 2);
}

fn switched_shop() -> (
    OwnShop,
    Disk,
    openbmt_core::identity::DeviceId,
    PinnedWallet,
) {
    let disk = Disk::new();
    let mut s = shop(SINGLE);
    let made = s.make_offer_as(T0, true).unwrap();
    let (_, request) = counter_request(&made, 3, "Front");
    let counter = request.counter;
    confirm(&mut s, &disk, &made, &request, T0 + 30).unwrap();
    let new = PinnedWallet::from_text(MULTI).unwrap();
    s.make_switch(counter, &new, T0 + 60, |b| disk.put(b))
        .unwrap();
    (s, disk, counter, new)
}

#[test]
fn ss_41_the_pinned_wallet_becomes_the_new_wallet_once_and_only_when_every_counter_switched() {
    use openbmt_owner::own_switch::{Repin, SwitchStatus};
    let (mut s, disk, counter, new) = switched_shop();
    let old = s.wallet().clone();
    let stored = disk.get();
    let other_counter = id(2).device_id();
    // A counter waiting or refused: nothing changes and nothing is stored.
    for status in [SwitchStatus::Waiting, SwitchStatus::Refused] {
        let rows = [(counter, SwitchStatus::Switched), (other_counter, status)];
        let out = s
            .repin(&new, &rows, |_: &[u8]| -> Result<(), String> {
                panic!("stored")
            })
            .unwrap();
        match out {
            Repin::NotReady(n) => assert_eq!(n.waiting.len() + n.refused.len(), 1),
            other => panic!("{other:?}"),
        }
        assert_eq!(s.wallet(), &old);
    }
    // A failed store changes nothing.
    let rows = [(counter, SwitchStatus::Switched)];
    let failed = s.repin(&new, &rows, |_: &[u8]| Err("disk full".to_string()));
    assert!(matches!(failed, Err(OwnError::Store(_))));
    assert_eq!(s.wallet(), &old);
    assert_eq!(disk.get(), stored);
    // Every counter switched: the new wallet is pinned, and stored for it.
    assert_eq!(s.repin(&new, &rows, |b| disk.put(b)).unwrap(), Repin::Done);
    assert_eq!(s.wallet(), &new);
    assert_ne!(disk.get(), stored);
    assert_eq!(s.to_bytes(), disk.get());
    // The state reads back for the new wallet and no longer for the old one,
    // and the book is the one it was.
    let back = OwnShop::restore(id(1), new.clone(), &disk.get()).unwrap();
    assert_eq!(back.wallet(), &new);
    assert_eq!(back.book(), s.book());
    // The old wallet's file over the new state is the crash between the two
    // writes: the shop finishes the re-pin. Another wallet's file does not.
    let finished = OwnShop::restore(id(1), old.clone(), &disk.get()).unwrap();
    assert!(finished.recovered_repin());
    assert_eq!(finished.wallet(), &new);
    // Once: asking again stores nothing and changes nothing, whatever the
    // counters say.
    let again = s
        .repin(&new, &[(counter, SwitchStatus::Waiting)], |_: &[u8]| {
            Err("stored again".to_string())
        })
        .unwrap();
    assert_eq!(again, Repin::AlreadyDone);
    assert_eq!(s.wallet(), &new);
    // The offer the shop makes now is for the new wallet.
    assert_eq!(s.make_offer(T0 + 90).unwrap().offer.wallet, new);
}

#[test]
fn ss_41_a_shop_with_no_counter_in_force_has_nothing_to_wait_for() {
    use openbmt_owner::own_switch::Repin;
    let mut s = shop(SINGLE);
    let new = PinnedWallet::from_text(MULTI).unwrap();
    let disk = Disk::new();
    assert_eq!(s.repin(&new, &[], |b| disk.put(b)).unwrap(), Repin::Done);
    assert_eq!(s.wallet(), &new);
}

/// A third wallet: another single-key account.
fn third() -> PinnedWallet {
    PinnedWallet::from_text("wpkh([37b5eed4/84h/0h/0h]xpub6BkU445MSEBXbPjD3g2c2ch6mn8yy1SXXQUM7EwjgYiq6Wt1NDwDZ45npqWcV8uQC5oi2gHuVukoCoZZyT4HKq8EpotPMqGqxdZRuapCQ23/<0;1>/*)#yhygl07f").unwrap()
}

#[test]
fn ss_42_the_move_pays_an_index_of_a_new_range_above_every_range_given() {
    let (mut s, disk, _, new) = switched_shop();
    // The counter holds 0 to 1,000 and its switch range 1,000 to 2,000.
    let first = s.move_index(&new, T0 + 100, |b| disk.put(b)).unwrap();
    assert_eq!(first, 2_000);
    assert_eq!(s.book().next_start(), 3_000);
    assert_eq!(s.book().device_of(first), Some(s.owner_id()));
    let back =
        OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), &disk.get()).unwrap();
    assert_eq!(back.book(), s.book());
}

#[test]
fn ss_42_the_move_index_is_taken_once_for_a_new_wallet_and_a_retry_gets_the_same() {
    let (mut s, disk, _, new) = switched_shop();
    let first = s.move_index(&new, T0 + 100, |b| disk.put(b)).unwrap();
    let book = s.book().clone();
    let stored = disk.get();
    // A retry of the signing: the same index, nothing allocated, nothing
    // stored.
    let again = s
        .move_index(&new, T0 + 900, |_: &[u8]| -> Result<(), String> {
            panic!("stored")
        })
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(s.book(), &book);
    assert_eq!(disk.get(), stored);
    // The plan is in the stored state: a shop restored from it answers the
    // same.
    let mut back =
        OwnShop::restore(id(1), PinnedWallet::from_text(SINGLE).unwrap(), &disk.get()).unwrap();
    let kept = back
        .move_index(&new, T0 + 950, |_: &[u8]| -> Result<(), String> {
            panic!("stored")
        })
        .unwrap();
    assert_eq!(kept, first);
    assert_eq!(back.book(), s.book());
}

#[test]
fn ss_42_another_new_wallet_is_refused_while_the_first_is_not_pinned_and_a_failed_store_takes_nothing(
) {
    use openbmt_owner::own_switch::{Repin, SwitchStatus};
    let (mut s, disk, counter, new) = switched_shop();
    // A failed store takes no range and plans nothing.
    let before = s.book().clone();
    let failed = s.move_index(&new, T0 + 100, |_: &[u8]| Err("disk full".to_string()));
    assert!(matches!(failed, Err(OwnError::Store(_))));
    assert_eq!(s.book(), &before);
    let first = s.move_index(&new, T0 + 110, |b| disk.put(b)).unwrap();
    assert_eq!(first, 2_000);
    // Another new wallet is refused, and takes nothing.
    let other = third();
    let book = s.book().clone();
    let r = refused(s.move_index(&other, T0 + 120, |b| disk.put(b)));
    assert_eq!(r, OwnRefusal::OtherNewWallet);
    assert_eq!(s.book(), &book);
    // Once the new wallet is pinned, the plan is done: the same wallet gets
    // the same index (a late payment's move), another gets a range of its own.
    let rows = [(counter, SwitchStatus::Switched)];
    assert_eq!(s.repin(&new, &rows, |b| disk.put(b)).unwrap(), Repin::Done);
    assert_eq!(
        s.move_index(&new, T0 + 130, |b| disk.put(b)).unwrap(),
        first
    );
    let next = s.move_index(&other, T0 + 140, |b| disk.put(b)).unwrap();
    assert_eq!(next, 3_000);
}

#[test]
fn ss_41_a_crash_at_each_point_of_the_repin_leaves_exactly_the_old_state_or_exactly_the_new_one() {
    use openbmt_owner::own_switch::{Repin, SwitchStatus};
    let (mut s, disk, counter, new) = switched_shop();
    let old = s.wallet().clone();
    let old_state = disk.get();
    let rows = [(counter, SwitchStatus::Switched)];
    // The wallet file the app keeps, and the state file: a crash writes none,
    // the state only, or both.
    let wallet_file = RefCell::new(old.to_bytes());
    // Before the own-shop write: the store fails, the files are the old ones.
    let failed = s.repin(&new, &rows, |_: &[u8]| Err("crash".to_string()));
    assert!(matches!(failed, Err(OwnError::Store(_))));
    let start = |state: &[u8], file: &[u8]| {
        OwnShop::restore(id(1), PinnedWallet::from_bytes(file).unwrap(), state)
    };
    let at = start(&old_state, &wallet_file.borrow()).unwrap();
    assert_eq!(at.wallet(), &old);
    assert!(!at.recovered_repin());
    assert_eq!(at.to_bytes(), old_state);
    // The re-pin: the own-shop write happens first.
    assert_eq!(s.repin(&new, &rows, |b| disk.put(b)).unwrap(), Repin::Done);
    let new_state = disk.get();
    // After the own-shop write, before the wallet write: the state names the
    // new wallet and the wallet file is the old one. The start finishes it.
    let at = start(&new_state, &wallet_file.borrow()).unwrap();
    assert_eq!(at.wallet(), &new);
    assert!(at.recovered_repin());
    assert_eq!(at.to_bytes(), new_state);
    assert_eq!(at.former_wallets().len(), 1);
    assert_eq!(at.former_wallets()[0].wallet, old);
    // The caller writes the wallet file; then the next start is a plain one.
    *wallet_file.borrow_mut() = at.wallet().to_bytes();
    let done = start(&new_state, &wallet_file.borrow()).unwrap();
    assert_eq!(done.wallet(), &new);
    assert!(!done.recovered_repin());
    assert_eq!(done.to_bytes(), new_state);
    // Any other mismatch stays refused: a third wallet's file, the old
    // wallet's file with the old state's record missing, another owner.
    assert!(matches!(
        start(&new_state, &third().to_bytes()),
        Err(OwnRestoreError::NotThisShop)
    ));
    assert!(matches!(
        OwnShop::restore(id(2), new.clone(), &new_state),
        Err(OwnRestoreError::NotThisShop)
    ));
    let no_record = {
        let mut other = shop(SINGLE);
        let d = Disk::new();
        other.allocate_desk(T0, |b| d.put(b)).unwrap();
        d.get()
    };
    assert!(matches!(
        start(&no_record, &new.to_bytes()),
        Err(OwnRestoreError::NotThisShop)
    ));
    // A state with a tail cut short is refused, as every cut is.
    for cut in 1..40 {
        assert!(start(&new_state[..new_state.len() - cut], &new.to_bytes()).is_err());
    }
}
