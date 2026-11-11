//! CR-196, CR-197 through `dispatch`: a sale is in the currency of the
//! settings, and Bitcoin Standard asks no price.

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
