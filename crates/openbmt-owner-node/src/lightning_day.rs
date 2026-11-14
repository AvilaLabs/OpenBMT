//! The suggestion for the cap of Shop Lightning (`docs/SHOP-LIGHTNING-RULES.md`
//! SL-34): one day's Lightning sales.
//!
//! The owner reads the sales the Lightning counter (SL-70) paid over
//! Lightning from its verified copy of the counter's records. A day is the 24
//! hours from midnight in the owner phone's time zone, whose offset from UTC
//! the app passes in. The suggestion is the most the counter took in one of
//! the last seven days, today included, and never more than the limit of
//! instant payments the settings allow. With no Lightning sale in those days
//! there is no suggestion.

use openbmt_core::identity::DeviceId;
use openbmt_core::records::{Chain, DeviceList, RecordKind};
use openbmt_core::settings::Settings;
use openbmt_counter::Event;

/// How many days, today included, the suggestion looks back over.
pub const SUGGEST_DAYS: u64 = 7;

const DAY_SECS: i64 = 86_400;

/// The most satoshis taken over Lightning in one local day of the last
/// [`SUGGEST_DAYS`] (`now` and each `sales` time are Unix seconds; `offset_secs`
/// is the zone's offset from UTC), at most `limit`. `None` with no sale in
/// those days, or when `limit` is 0.
pub fn suggest_cap(sales: &[(u64, u64)], now: u64, offset_secs: i32, limit: u64) -> Option<u64> {
    let day_of = |t: u64| {
        (i64::try_from(t).unwrap_or(i64::MAX) + i64::from(offset_secs)).div_euclid(DAY_SECS)
    };
    let today = day_of(now);
    let first = today - (SUGGEST_DAYS as i64 - 1);
    let mut by_day: std::collections::BTreeMap<i64, u64> = std::collections::BTreeMap::new();
    for &(at, sats) in sales {
        let d = day_of(at);
        if (first..=today).contains(&d) {
            let total = by_day.entry(d).or_insert(0);
            *total = total.saturating_add(sats);
        }
    }
    let best = by_day.values().copied().max().filter(|&b| b > 0)?;
    (limit > 0).then(|| best.min(limit))
}

/// The most the settings let the counters hold at risk at once: the own
/// node's shop-wide outstanding instant total (CR-15), or the own node's
/// largest instant sale when the settings give no total.
pub fn instant_limit(settings: &Settings) -> u64 {
    match settings.node_budget.sats {
        0 => settings.node_per_sale.sats,
        total => total,
    }
}

/// The sales `counter` took over Lightning, from a copy of its records that
/// verifies, as `(time, satoshis)`. A copy that does not verify, or a record
/// that does not read, gives none.
pub fn lightning_sales(chain: &Chain, devices: &DeviceList, counter: &DeviceId) -> Vec<(u64, u64)> {
    if chain.verify(devices).is_err() {
        return Vec::new();
    }
    let mut sales = Vec::new();
    for record in chain.records() {
        if &record.author != counter || record.kind != RecordKind::Payment {
            continue;
        }
        match Event::decode(&record.body) {
            Ok(Event::PaidByLightning {
                at, amount_msat, ..
            }) => sales.push((at, amount_msat / 1_000)),
            Ok(_) => {}
            Err(_) => return Vec::new(),
        }
    }
    sales
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbmt_core::identity::Identity;
    use openbmt_counter::Amount;

    const DAY: u64 = 86_400;
    const NOW: u64 = 1_800_000_000 - 1_800_000_000 % DAY + 12 * 3_600;

    #[test]
    fn sl_34_no_lightning_sales_give_no_suggestion() {
        assert_eq!(suggest_cap(&[], NOW, 0, 1_000_000), None);
    }

    #[test]
    fn sl_34_one_day_is_the_total_of_that_day() {
        let sales = [(NOW - 100, 4_000), (NOW - 200, 6_000)];
        assert_eq!(suggest_cap(&sales, NOW, 0, 1_000_000), Some(10_000));
    }

    #[test]
    fn sl_34_the_largest_of_several_days_is_taken() {
        let sales = [
            (NOW, 1_000),
            (NOW - DAY, 3_000),
            (NOW - DAY + 5, 4_000),
            (NOW - 3 * DAY, 5_000),
            (NOW - 6 * DAY, 2_000),
        ];
        assert_eq!(suggest_cap(&sales, NOW, 0, 1_000_000), Some(7_000));
    }

    #[test]
    fn sl_34_days_older_than_seven_are_ignored() {
        let sales = [(NOW - 7 * DAY, 9_000_000), (NOW - 6 * DAY, 500)];
        assert_eq!(suggest_cap(&sales, NOW, 0, 1_000_000), Some(500));
        assert_eq!(suggest_cap(&[(NOW - 7 * DAY, 9)], NOW, 0, 1_000_000), None);
    }

    #[test]
    fn sl_34_the_suggestion_is_at_most_the_limit() {
        assert_eq!(suggest_cap(&[(NOW, 50_000)], NOW, 0, 20_000), Some(20_000));
        assert_eq!(suggest_cap(&[(NOW, 50_000)], NOW, 0, 0), None);
    }

    #[test]
    fn sl_34_a_day_is_cut_at_local_midnight() {
        // 23:00 UTC and 01:00 UTC next day: one day at +2h, two at UTC.
        let late = NOW - 12 * 3_600 + 23 * 3_600 - DAY;
        let sales = [(late, 1_000), (late + 2 * 3_600, 2_000)];
        assert_eq!(suggest_cap(&sales, NOW, 0, 1_000_000), Some(2_000));
        assert_eq!(suggest_cap(&sales, NOW, 2 * 3_600, 1_000_000), Some(3_000));
        // An hour west of UTC they are split again: 22:00 and 00:00 local.
        assert_eq!(suggest_cap(&sales, NOW, -3_600, 1_000_000), Some(2_000));
    }

    #[test]
    fn sl_34_the_limit_is_the_total_or_else_the_largest_sale() {
        let mut s = Settings::defaults(1_000);
        s.node_budget.sats = 70_000;
        s.node_per_sale.sats = 9_000;
        assert_eq!(instant_limit(&s), 70_000);
        s.node_budget.sats = 0;
        assert_eq!(instant_limit(&s), 9_000);
    }

    fn lightning(index: u32, at: u64, sats: u64) -> Event {
        Event::PaidByLightning {
            index,
            at,
            payment_hash: [index as u8; 32],
            amount_msat: sats * 1_000,
            settled_at: None,
            valued: Amount(0),
        }
    }

    fn on_chain(index: u32, at: u64, sats: u64) -> Event {
        Event::LatePayment {
            index,
            at,
            txid: [7; 32],
            sats,
        }
    }

    fn late(index: u32, at: u64, sats: u64) -> Event {
        Event::LateLightning {
            index,
            at,
            payment_hash: [9; 32],
            amount_msat: sats * 1_000,
        }
    }

    fn id(n: u8) -> Identity {
        Identity::from_secret_bytes(&[n; 32]).unwrap()
    }

    fn chain(author: &Identity, events: &[Event]) -> Chain {
        let mut c = Chain::new();
        for e in events {
            c.append(author, e.kind(), e.encode()).unwrap();
        }
        c
    }

    #[test]
    fn sl_70_only_lightning_sales_of_the_lightning_counter_count() {
        let (me, other) = (id(3), id(4));
        let devices = DeviceList::new([me.device_id(), other.device_id()]);
        let mine = chain(
            &me,
            &[
                lightning(0, NOW, 700),
                late(1, NOW, 5_000),
                on_chain(2, NOW, 8_000),
            ],
        );
        assert_eq!(
            lightning_sales(&mine, &devices, &me.device_id()),
            vec![(NOW, 700)]
        );
        // A sale at another counter is not the Lightning counter's.
        let theirs = chain(&other, &[lightning(0, NOW, 900)]);
        assert!(lightning_sales(&theirs, &devices, &me.device_id()).is_empty());
        // A copy that does not verify gives none.
        assert!(lightning_sales(&mine, &DeviceList::new([]), &me.device_id()).is_empty());
    }
}
