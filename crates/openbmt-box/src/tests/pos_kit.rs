//! What the POS tests share: a connector the test controls, the shop's POS
//! settings, notices and orders.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::DeviceRole;
use openbmt_counter::{Amount, Currency};
use openbmt_pos::{
    CloseOutcome, CloseRequest, Connector, Cursor, Id, LocationCounters, NoticeDraft, NoticeState,
    OrderId, OrderState, Page, PaidNotice, PosError, PosOrder, PosReader, PosTender, SaleFact,
    Settings as PosSettings, TenderKind, TenderState, Text,
};

use super::kit::*;
use crate::files::MemFiles;
use crate::pos::{BoxPos, PosSetup};

/// What a test connector was asked and what it answers next.
#[derive(Default)]
pub struct Probe {
    pub pages: VecDeque<Result<Page, PosError>>,
    pub polls: Vec<Cursor>,
    pub tokens: Vec<Option<String>>,
    pub can_close: bool,
    pub order: Option<OrderId>,
    pub find_calls: usize,
    /// The sale indexes `find_order` was asked for, in order.
    pub asked: Vec<u32>,
    pub close_script: VecDeque<Result<CloseOutcome, PosError>>,
    pub closes: Vec<CloseRequest>,
}

/// A connector whose state the test holds a handle to.
#[derive(Clone, Default)]
pub struct TestConnector(pub Arc<Mutex<Probe>>);

impl TestConnector {
    pub fn probe(&self) -> MutexGuard<'_, Probe> {
        self.0.lock().unwrap()
    }

    /// A connector that can close the order `order`.
    pub fn closing(order: &str) -> Self {
        let c = Self::default();
        {
            let mut p = c.probe();
            p.can_close = true;
            p.order = Some(Id::new(order).unwrap());
        }
        c
    }
}

impl PosReader for TestConnector {
    fn poll(&mut self, cursor: &Cursor) -> Result<Page, PosError> {
        let mut p = self.probe();
        p.polls.push(cursor.clone());
        p.pages.pop_front().unwrap_or_else(|| {
            Ok(Page {
                orders: Vec::new(),
                next: cursor.clone(),
                more: false,
            })
        })
    }
}

impl Connector for TestConnector {
    fn set_token(&mut self, token: Option<&str>) {
        self.probe().tokens.push(token.map(str::to_string));
    }

    fn can_close(&self) -> bool {
        self.probe().can_close
    }

    fn find_order(&mut self, sale: &SaleFact) -> Result<Option<OrderId>, PosError> {
        let mut p = self.probe();
        p.find_calls += 1;
        p.asked.push(sale.index);
        Ok(p.order.clone())
    }

    fn close(&mut self, request: &CloseRequest) -> Result<CloseOutcome, PosError> {
        let mut p = self.probe();
        p.closes.push(request.clone());
        p.close_script
            .pop_front()
            .unwrap_or(Ok(CloseOutcome::Closed))
    }
}

pub fn usd() -> Currency {
    Currency::new(*b"USD").unwrap()
}

/// The shop's POS settings: one location, served by counter 1.
pub fn pos_settings() -> PosSettings {
    PosSettings::new(
        vec![Text::new("Bitcoin").unwrap()],
        vec![LocationCounters {
            location: Text::new("Main").unwrap(),
            counters: vec![counter1().device_id()],
        }],
        vec![usd()],
    )
    .unwrap()
}

/// The roles the settings in force give: the two counters and the owner.
pub fn roles() -> BTreeMap<DeviceId, DeviceRole> {
    [
        (counter1().device_id(), DeviceRole::Counter),
        (counter2().device_id(), DeviceRole::Counter),
        (owner().device_id(), DeviceRole::Owner),
    ]
    .into_iter()
    .collect()
}

pub fn draft(index: u32, state: NoticeState, at: u64, cents: u64) -> NoticeDraft {
    NoticeDraft {
        index,
        amount: Amount(cents),
        currency: usd(),
        opened_at: NOW,
        state,
        state_at: at,
        till_sale: None,
    }
}

pub fn notice(by: &Identity, number: u64, d: &NoticeDraft) -> PaidNotice {
    PaidNotice::sign(by, number, d).unwrap()
}

/// The bytes of a notice as the sender `by` delivers it.
pub fn item(by: &Identity, number: u64, d: &NoticeDraft) -> (DeviceId, Vec<u8>) {
    (by.device_id(), notice(by, number, d).to_bytes())
}

/// A closed order with one Bitcoin tender.
pub fn order(id: &str, cents: u64, at: u64) -> PosOrder {
    PosOrder {
        id: Id::new(id).unwrap(),
        location: Text::new("Main").unwrap(),
        state: OrderState::Closed,
        currency: usd(),
        total_minor: cents,
        tenders: vec![PosTender {
            id: Id::new(&format!("t-{id}")).unwrap(),
            kind: TenderKind::Other,
            name: Text::new("Bitcoin").unwrap(),
            amount_minor: cents,
            tip_minor: 0,
            state: TenderState::Paid,
            at,
            reference: None,
        }],
        opened_at: at,
        closed_at: Some(at),
        updated_at: at,
        reference: None,
    }
}

pub fn page(orders: Vec<PosOrder>) -> Page {
    Page {
        orders,
        next: Cursor::new(b"next".to_vec()).unwrap(),
        more: false,
    }
}

/// A POS side over files in memory with this connector, reading every 30
/// seconds.
pub fn pos_over(files: MemFiles, connector: &TestConnector) -> BoxPos<MemFiles> {
    BoxPos::open(
        files,
        the_box(),
        PosSetup::new(pos_settings(), Box::new(connector.clone())),
    )
    .unwrap()
}

pub fn pos(connector: &TestConnector) -> BoxPos<MemFiles> {
    pos_over(MemFiles::new(), connector)
}

pub fn token() -> Result<Option<String>, String> {
    Ok(Some("pos-token-test".to_string()))
}
