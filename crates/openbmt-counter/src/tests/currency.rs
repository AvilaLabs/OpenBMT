//! CR-196, CR-197: the shop's currency prices a sale; Bitcoin Standard
//! prices in satoshis and asks no source.

use openbmt_core::currency::ShopCurrency;
use openbmt_core::settings::Limit;

use super::kit::*;
use crate::money::{value_of, SATS_LABEL};
use crate::{Amount, Currency, Event, OpenRefused, Rate, RateSource};

/// A world whose settings are in `currency`. For Bitcoin Standard every
/// limit is its own satoshis (CR-194).
fn world_in(currency: ShopCurrency) -> World {
    let mut w = World::new();
    let mut s = settings();
    s.currency = currency;
    if currency.is_sats() {
        for l in [
            &mut s.light_per_sale,
            &mut s.light_budget,
            &mut s.node_per_sale,
            &mut s.node_budget,
            &mut s.box_per_sale,
            &mut s.box_budget,
            &mut s.refund_cap,
            &mut s.refund_daily_cap,
            &mut s.cashier_override_daily_cap,
            &mut s.manager_override_ceiling,
        ] {
            *l = Limit {
                sats: 10_000,
                display_usd: 10_000,
            };
        }
    }
    assert_eq!(s.validate(), Ok(()));
    w.c.set_settings(s);
    w
}

fn opened_event(w: &World) -> Event {
    w.m.events()
        .into_iter()
        .find(|e| matches!(e, Event::Opened { .. }))
        .expect("an Opened record")
}

#[test]
fn cr_196_a_sale_is_in_the_currency_of_the_settings_whatever_the_caller_says() {
    for currency in ShopCurrency::ALL {
        if currency.is_sats() {
            continue;
        }
        let mut w = world_in(currency);
        // The caller says dollars: the sale is in the shop's currency.
        let o = w.open_five();
        let want = Currency::of_shop(currency);
        assert_eq!(w.c.sale(o.index).unwrap().currency(), want);
        assert_eq!(want.to_string(), currency.code());
        match opened_event(&w) {
            Event::Opened {
                currency: got,
                minor_per_btc,
                source,
                ..
            } => {
                assert_eq!(got, want);
                // The rate is the caller's: a shop with a price asks one.
                assert_eq!(minor_per_btc, rate().minor_per_btc());
                assert_eq!(source.label(), "test-fixed");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn cr_196_the_currency_of_a_shop_is_a_three_capital_letter_code() {
    for currency in ShopCurrency::ALL {
        let c = Currency::of_shop(currency);
        assert_eq!(
            c.code(),
            <[u8; 3]>::try_from(currency.code().as_bytes()).unwrap()
        );
        assert!(Currency::new(c.code()).is_some());
    }
    assert_eq!(Currency::of_shop(ShopCurrency::Sats).to_string(), "SAT");
}

#[test]
fn cr_197_bitcoin_standard_prices_the_amount_as_its_own_satoshis() {
    let mut w = world_in(ShopCurrency::Sats);
    // 5,000 sats typed; the caller's rate would make it 50,000 sats, and
    // its source is long stale: neither is read.
    let facts = w.open_facts();
    let stale = RateSource::new("test-fixed", 1).unwrap();
    let o =
        w.c.open(Amount(5_000), usd(), rate(), &stale, facts, &mut w.m)
            .expect("a sale opens with no price");
    assert_eq!(o.needs_sats, 5_000);
    assert_eq!(o.quote.rate, Rate::sats());
    assert_eq!(o.quote.rate.minor_per_btc(), 100_000_000);
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(sale.currency().to_string(), "SAT");
    assert_eq!(sale.amount(), Amount(5_000));
    // The sale records no price: the label says there was none.
    match opened_event(&w) {
        Event::Opened {
            currency,
            amount,
            needs_sats,
            minor_per_btc,
            source,
            ..
        } => {
            assert_eq!(currency.to_string(), "SAT");
            assert_eq!(amount, Amount(5_000));
            assert_eq!(needs_sats, 5_000);
            assert_eq!(minor_per_btc, 100_000_000);
            assert_eq!(source.label(), SATS_LABEL);
            assert_eq!(source.label(), "sats");
            assert_eq!(source.fetched_at(), w.now);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn cr_197_a_satoshi_is_worth_a_minor_unit_at_the_rate_of_bitcoin_standard() {
    for sats in [1, 546, 5_000, 123_456_789, 2_100_000_000_000_000] {
        assert_eq!(value_of(sats, Rate::sats()), Amount(sats));
        assert_eq!(crate::sats_for(Amount(sats), Rate::sats()), Some(sats));
    }
    assert_eq!(RateSource::sats(7).label(), "sats");
    assert_eq!(RateSource::sats(7).fetched_at(), 7);
    assert!(RateSource::sats(7).is_fresh_at(7));
}

#[test]
fn cr_197_the_dust_line_and_the_floor_still_apply_in_satoshis() {
    let mut w = world_in(ShopCurrency::Sats);
    let facts = w.open_facts();
    let err =
        w.c.open(
            Amount(400),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            &mut w.m,
        )
        .unwrap_err();
    assert_eq!(err, OpenRefused::BelowDust { needs_sats: 400 });
    let mut s = w.c.settings().clone();
    s.min_sale_sats = 2_000;
    w.c.set_settings(s);
    let facts = w.open_facts();
    let err =
        w.c.open(
            Amount(1_999),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            &mut w.m,
        )
        .unwrap_err();
    assert_eq!(
        err,
        OpenRefused::BelowMinimum {
            sats: 1_999,
            min_sats: 2_000
        }
    );
}

#[test]
fn cr_197_a_new_quote_in_bitcoin_standard_is_the_amount_again() {
    let mut w = world_in(ShopCurrency::Sats);
    let o = w.open(5_000);
    w.tick(60);
    // A price the caller makes up is not read.
    let q = w.requote(o.index, 20_000_000).expect("requoted");
    assert_eq!(q.rate, Rate::sats());
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(sale.quotes().last().map(|q| q.rate), Some(Rate::sats()));
    match w.m.events().last() {
        Some(Event::Requoted {
            needs_sats,
            minor_per_btc,
            source,
            ..
        }) => {
            assert_eq!(*needs_sats, 5_000);
            assert_eq!(*minor_per_btc, 100_000_000);
            assert_eq!(source.label(), "sats");
        }
        other => panic!("{other:?}"),
    }
}
