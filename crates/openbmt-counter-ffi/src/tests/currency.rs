//! CR-196, CR-197 through `dispatch`: a sale is in the currency of the
//! settings, and Bitcoin Standard asks no price.

use bitcoin::{Address, Network};
use openbmt_core::currency::ShopCurrency;
use openbmt_core::settings::{Limit, SignedSettings};

use super::kit::*;
use super::wire::{Rd, W};

/// The test settings with the shop in `currency`.
fn signed_in(currency: ShopCurrency) -> SignedSettings {
    let mut s = signed_settings().settings;
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
                sats: 20_000,
                display_usd: 20_000,
            };
        }
    }
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

/// A started node of a shop in `currency` that has heard from the network.
fn ready_in(currency: ShopCurrency) -> (Rig, u64) {
    let rig = Rig::new();
    let dir = rig.dir();
    let mut config = start_config(dir.to_str().expect("UTF-8"));
    config.record = record_with(signed_in(currency)).to_bytes();
    let handle = rig.start_with(&config);
    rig.send(vec![peers_event()]);
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    r.pump();
    r.done();
    (rig, handle)
}

fn shop_currency(rig: &Rig) -> String {
    let record = record_with(signed_in(ShopCurrency::Eur)).to_bytes();
    let dir = rig.dir();
    let response = rig.call(
        43,
        0,
        &W::new()
            .bytes(&record)
            .string(dir.to_str().expect("UTF-8"))
            .0,
    );
    let mut r = Rd::ok(&response);
    let _network = r.u8();
    let _version = r.u32();
    let _tor = r.u8();
    let _wifi = r.u8();
    let _floor = r.u64();
    r.string()
}

#[test]
fn cr_193_the_settings_view_names_the_shops_currency() {
    let rig = Rig::new();
    assert_eq!(shop_currency(&rig), "EUR");
}

#[test]
fn cr_196_a_sale_in_another_currency_than_the_shops_is_refused() {
    let (rig, handle) = ready_in(ShopCurrency::Eur);
    let sale = |code: &str| {
        rig.call(
            11,
            handle,
            &W::new().open(T0, 500, code, RATE, "test-fixed").0,
        )
    };
    let f = Rd::error(&sale("USD")).expect("a refusal");
    assert_eq!(f.rule, "sale-currency");
    let f = Rd::error(&sale("SAT")).expect("a refusal");
    assert_eq!(f.rule, "sale-currency");
    // In the shop's currency it opens, at the price given: 1,000 sats a unit.
    let response = sale("EUR");
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    assert_eq!(opened.needs_sats, 5_000);
    assert_eq!(opened.quote_rate, RATE);
}

#[test]
fn cr_197_a_bitcoin_standard_sale_takes_no_price_and_no_source() {
    let (rig, handle) = ready_in(ShopCurrency::Sats);
    // A rate of zero and a label that is not one: neither is read.
    let response = rig.call(11, handle, &W::new().open(T0, 7_500, "SAT", 0, "").0);
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    assert_eq!(opened.needs_sats, 7_500);
    assert_eq!(opened.quote_rate, 100_000_000);
    // The shop's currency is the only one that opens.
    let f = Rd::error(&rig.call(
        11,
        handle,
        &W::new().open(T0, 7_500, "USD", RATE, "test-fixed").0,
    ))
    .expect("a refusal");
    assert_eq!(f.rule, "sale-currency");
}

/// The arguments of `requote` for sale 0.
fn requote_args(now: u64, currency: &str, rate: u64, label: &str) -> Vec<u8> {
    W::new()
        .u64(now)
        .u32(0)
        .string(currency)
        .u64(rate)
        .string(label)
        .u64(now)
        .0
}

#[test]
fn cr_196_a_requote_in_another_currency_than_the_sales_is_refused() {
    let (rig, handle) = ready_in(ShopCurrency::Eur);
    let sale = rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "EUR", RATE, "test-fixed").0,
    );
    let mut r = Rd::ok(&sale);
    let opened = r.opened();
    r.done();
    assert_eq!(opened.needs_sats, 5_000);
    // A rate of dollars, or of nothing, is not the sale's.
    for named in ["USD", "SAT", "eur", "EU"] {
        let f = Rd::error(&rig.call(
            13,
            handle,
            &requote_args(T0 + 1, named, RATE * 2, "test-fixed"),
        ))
        .expect("a refusal");
        // The library refuses another currency; letters that are not one
        // never reach it.
        let rule = if named.len() == 3 && named.chars().all(char::is_uppercase) {
            "CR-196"
        } else {
            "sale-currency"
        };
        assert_eq!(f.rule, rule, "{named}");
    }
    // The sale still needs what it did; the sale's own currency requotes.
    let response = rig.call(
        13,
        handle,
        &requote_args(T0 + 1, "EUR", RATE * 2, "test-fixed"),
    );
    let mut r = Rd::ok(&response);
    let quote = r.opened();
    r.done();
    assert_eq!(quote.needs_sats, 2_500);
}

#[test]
fn cr_196_a_refund_by_value_in_another_currency_than_the_sales_is_refused() {
    let (rig, handle) = ready_in(ShopCurrency::Eur);
    rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "EUR", RATE, "test-fixed").0,
    );
    let destination = Address::from_script(&p2wsh(0x77), Network::Regtest)
        .expect("an address")
        .to_string();
    let ask = |currency: &str| {
        W::new()
            .u64(T0)
            .u32(0)
            .u8(1)
            .u64(100)
            .string(currency)
            .u64(RATE)
            .string("test-fixed")
            .u64(T0)
            .string(&destination)
            .actor(true, "mia")
            .string("changed mind")
            .0
    };
    for named in ["USD", "SAT"] {
        let f = Rd::error(&rig.call(16, handle, &ask(named))).expect("a refusal");
        assert_eq!(f.rule, "CR-196", "{named}");
    }
    // In the sale's currency the ask goes on to the refund rules, which
    // refuse it for a sale that has not been paid, not for its currency.
    let f = Rd::error(&rig.call(16, handle, &ask("EUR"))).expect("a refusal");
    assert_ne!(f.rule, "CR-196");
}

#[test]
fn cr_197_a_requote_in_bitcoin_standard_takes_no_price_and_no_source() {
    let (rig, handle) = ready_in(ShopCurrency::Sats);
    let response = rig.call(11, handle, &W::new().open(T0, 7_500, "SAT", 0, "").0);
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    assert_eq!(opened.needs_sats, 7_500);
    // The app sends a rate of zero and a blank source, as for `open`.
    let response = rig.call(13, handle, &requote_args(T0 + 1, "SAT", 0, ""));
    let mut r = Rd::ok(&response);
    let quote = r.opened();
    r.done();
    assert_eq!(quote.needs_sats, 7_500);
    assert_eq!(quote.quote_rate, 100_000_000);
    // A rate that is made up is not read either.
    let response = rig.call(13, handle, &requote_args(T0 + 2, "SAT", RATE * 2, "x"));
    let mut r = Rd::ok(&response);
    assert_eq!(r.opened().quote_rate, 100_000_000);
    r.done();
    // The currency is still the sale's.
    let f =
        Rd::error(&rig.call(13, handle, &requote_args(T0 + 3, "USD", 0, ""))).expect("a refusal");
    assert_eq!(f.rule, "CR-196");
}

#[test]
fn cr_05_a_requote_of_a_rate_of_zero_in_a_shop_with_a_price_is_still_refused() {
    let (rig, handle) = ready_in(ShopCurrency::Eur);
    rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "EUR", RATE, "test-fixed").0,
    );
    let f = Rd::error(&rig.call(13, handle, &requote_args(T0 + 1, "EUR", 0, "test-fixed")))
        .expect("a refusal");
    assert_eq!(f.rule, "sale-rate");
    let f = Rd::error(&rig.call(13, handle, &requote_args(T0 + 1, "EUR", RATE, "")))
        .expect("a refusal");
    assert_eq!(f.rule, "sale-source");
}

#[test]
fn cr_196_a_cash_refund_in_another_currency_than_the_sales_is_refused() {
    let (rig, handle) = ready_in(ShopCurrency::Eur);
    rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "EUR", RATE, "test-fixed").0,
    );
    let cash = |currency: &str| {
        W::new()
            .u64(T0)
            .u32(0)
            .actor(true, "mia")
            .u64(100)
            .string(currency)
            .string("till")
            .0
    };
    for named in ["USD", "SAT"] {
        let f = Rd::error(&rig.call(18, handle, &cash(named))).expect("a refusal");
        assert_eq!(f.rule, "CR-196", "{named}");
    }
    // Letters that are not a currency never reach the library.
    let f = Rd::error(&rig.call(18, handle, &cash("eu"))).expect("a refusal");
    assert_eq!(f.rule, "sale-currency");
    // Nothing was written: the sale has no refund.
    let response = rig.call(19, handle, &W::new().u32(0).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.list(|r| r.u8()).len(), 0);
    // In the sale's currency it goes on to the refund rules, and an unpaid
    // sale has nothing to give back, which is not its currency.
    let f = Rd::error(&rig.call(18, handle, &cash("EUR"))).expect("a refusal");
    assert_ne!(f.rule, "CR-196");
}

/// A node that was started in euros, has a euro sale open, and is started
/// again on the same folder with the shop in dollars: the sale is in euros,
/// the settings in force are in dollars.
fn euro_sale_under_dollar_settings() -> (Rig, u64) {
    let (rig, handle) = ready_in(ShopCurrency::Eur);
    rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "EUR", RATE, "test-fixed").0,
    );
    let _ = rig.call(2, handle, &[]);
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.record = record_with(signed_in(ShopCurrency::Usd)).to_bytes();
    let handle = rig.start_with(&config);
    (rig, handle)
}

#[test]
fn cr_196_a_requote_of_a_sale_in_the_old_currency_is_refused_after_the_shop_changes() {
    let (rig, handle) = euro_sale_under_dollar_settings();
    // The sale is in euros, the caller names euros, and the rate is dollars.
    let f = Rd::error(&rig.call(
        13,
        handle,
        &requote_args(T0 + 1, "EUR", RATE * 2, "test-fixed"),
    ))
    .expect("a refusal");
    assert_eq!(f.rule, "sale-currency");
}

#[test]
fn cr_196_a_refund_by_value_of_a_sale_in_the_old_currency_is_refused_after_the_shop_changes() {
    let (rig, handle) = euro_sale_under_dollar_settings();
    let destination = Address::from_script(&p2wsh(0x77), Network::Regtest)
        .expect("an address")
        .to_string();
    let f = Rd::error(
        &rig.call(
            16,
            handle,
            &W::new()
                .u64(T0)
                .u32(0)
                .u8(1)
                .u64(100)
                .string("EUR")
                .u64(RATE)
                .string("test-fixed")
                .u64(T0)
                .string(&destination)
                .actor(true, "mia")
                .string("changed mind")
                .0,
        ),
    )
    .expect("a refusal");
    assert_eq!(f.rule, "sale-currency");
}
