//! A counter's chain with a spread of sales, and helpers the tests share.
//! The values are distinctive on purpose, so that a scan of an export for
//! them cannot hit by coincidence.

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::{Chain, DeviceList};
use openbmt_counter::{Amount, Currency, Event, Role};

use crate::{Books, SaleKey};

pub const JAN_31_235930: u64 = 1_738_367_970;
pub const FEB_1_0100: u64 = 1_738_371_600;
pub const FEB_2_NOON: u64 = 1_738_497_600;

pub const TXID_LATE: [u8; 32] = [0xcd; 32];
pub const SATS_PAID: u64 = 4_321_987;
pub const SATS_LATE: u64 = 7_654_321;
pub const SATS_SURPLUS: u64 = 1_111_117;
pub const SATS_DUST: u64 = 3_999_017;
pub const SATS_REFUND: u64 = 3_141_593;
pub const NASTY_NAME: &str = "=cmd|' /C calc'!A0";

pub fn identity(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).expect("a valid secret")
}

pub fn usd() -> Currency {
    Currency::new(*b"USD").expect("a currency")
}

pub fn opened(index: u32, at: u64, cents: u64) -> Event {
    Event::Opened {
        index,
        at,
        amount: Amount(cents),
        currency: usd(),
        needs_sats: 4_321_987,
        minor_per_btc: 4_281_700,
    }
}

pub fn write(chain: &mut Chain, who: &Identity, event: &Event) {
    chain
        .append(who, event.kind(), event.encode())
        .expect("appended");
}

pub struct Kit {
    pub counter: Identity,
    pub owner: Identity,
    pub devices: DeviceList,
    pub chain: Chain,
}

impl Kit {
    pub fn counter_id(&self) -> DeviceId {
        self.counter.device_id()
    }

    pub fn key(&self, index: u32) -> SaleKey {
        SaleKey {
            counter: self.counter_id(),
            index,
        }
    }

    pub fn books(&self) -> Books {
        Books::from_chains(&self.devices, &[(self.counter_id(), &self.chain)]).expect("books")
    }
}

/// A counter and an owner on one device list, and a chain of the counter.
pub fn kit_with(events: &[Event]) -> Kit {
    let counter = identity(1);
    let owner = identity(2);
    let devices = DeviceList::new([counter.device_id(), owner.device_id()]);
    let mut chain = Chain::new();
    for e in events {
        write(&mut chain, &counter, e);
    }
    Kit {
        counter,
        owner,
        devices,
        chain,
    }
}

/// Eight sales in every state the counter writes.
pub fn rich_events() -> Vec<Event> {
    vec![
        // 0: paid at once, confirmed, final. Funds first recorded on 31 Jan.
        opened(0, JAN_31_235930 - 60, 1850),
        Event::PaidAtOnce {
            index: 0,
            at: JAN_31_235930,
            at_risk_sats: SATS_PAID,
            label: openbmt_core::policy::InstantLabel::YourNode,
            mode: openbmt_core::policy::CheckMode::NodeAlsoSeenByLight,
        },
        Event::Confirmed {
            index: 0,
            at: JAN_31_235930 + 600,
            received_sats: SATS_PAID,
            depth: 1,
        },
        Event::Final {
            index: 0,
            at: FEB_1_0100,
            received_sats: SATS_PAID,
            depth: 6,
        },
        // 1: expired.
        opened(1, FEB_1_0100, 500),
        Event::Expired {
            index: 1,
            at: FEB_1_0100 + 300,
        },
        // 2: cancelled.
        opened(2, FEB_1_0100, 700),
        Event::Cancelled {
            index: 2,
            at: FEB_1_0100 + 20,
        },
        // 3: expired, then a late payment.
        opened(3, FEB_1_0100, 900),
        Event::Expired {
            index: 3,
            at: FEB_1_0100 + 300,
        },
        Event::LatePayment {
            index: 3,
            at: FEB_1_0100 + 4000,
            txid: TXID_LATE,
            sats: SATS_LATE,
        },
        // 4: confirmed, not final.
        opened(4, FEB_2_NOON - 100, 2500),
        Event::Confirmed {
            index: 4,
            at: FEB_2_NOON,
            received_sats: SATS_PAID,
            depth: 1,
        },
        // 5: final, overpaid, with an override and the POS told.
        opened(5, FEB_2_NOON, 4200),
        Event::Override {
            index: 5,
            at: FEB_2_NOON + 5,
            role: Role::Manager,
            name: NASTY_NAME.to_string(),
            sats: SATS_PAID,
        },
        Event::Final {
            index: 5,
            at: FEB_2_NOON + 900,
            received_sats: SATS_PAID + SATS_SURPLUS,
            depth: 6,
        },
        Event::SurplusOwed {
            index: 5,
            at: FEB_2_NOON + 901,
            sats: SATS_SURPLUS,
        },
        Event::PosPaid {
            index: 5,
            at: FEB_2_NOON + 902,
        },
        // 6: paid at once, then replaced.
        opened(6, FEB_2_NOON, 6100),
        Event::PaidAtOnce {
            index: 6,
            at: FEB_2_NOON + 10,
            at_risk_sats: SATS_PAID,
            label: openbmt_core::policy::InstantLabel::LightCheck,
            mode: openbmt_core::policy::CheckMode::Light,
        },
        Event::Replaced {
            index: 6,
            at: FEB_2_NOON + 400,
        },
        // 7: dust only.
        opened(7, FEB_2_NOON, 100),
        Event::Dust {
            index: 7,
            at: FEB_2_NOON + 50,
            sats: SATS_DUST,
        },
    ]
}

pub fn rich_kit() -> Kit {
    kit_with(&rich_events())
}
