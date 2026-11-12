//! The counter's bills and tabs (CR-246 to CR-253): opening, parts, counting,
//! settling, tab changes, closing short and cancelling.
//!
//! Every step writes its `Sale` record first and the bills file after it
//! (CR-247), as a quote is written (CR-59): if the process stops between the
//! two, the record names a state the file does not hold, and the step is
//! asked for again. A bill's number is stored before it is used (CR-246).

use super::*;
use crate::bills::{
    reference_ok, split_shares, Bill, BillEnd, BillKind, BillPart, Ended, PartWay, MAX_BILL_AMOUNT,
    MAX_BILL_PARTS, MAX_OPEN_BILLS,
};
use crate::refusal::PartRefused;

/// A bill with what it has received, as the counter works it out (CR-249).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillView {
    /// The bill.
    pub bill: Bill,
    /// What the bitcoin parts that count are worth, in minor units.
    pub received_bitcoin: u64,
    /// What the parts paid another way came to, in minor units.
    pub received_other: u64,
    /// Bitcoin parts that are not paid yet and may still be.
    pub pending: usize,
    /// What is left of the amount, in minor units (0 when it is reached).
    pub left: u64,
}

impl BillView {
    /// What the bill has received in all.
    pub fn received(&self) -> u64 {
        self.received_bitcoin.saturating_add(self.received_other)
    }
}

impl Counter {
    /// CR-249: what a bill has received and how many parts are pending.
    fn bill_totals(&self, bill: &Bill) -> (u64, u64, usize) {
        let (mut bitcoin, mut other, mut pending) = (0u64, 0u64, 0usize);
        for part in &bill.parts {
            match part {
                BillPart::Sale(index) => match self.sales.get(index) {
                    Some(sale) if sale.pos == PosState::Paid => {
                        bitcoin = bitcoin.saturating_add(sale.own_amount().0);
                    }
                    Some(sale) if sale.is_live() => pending += 1,
                    _ => {}
                },
                BillPart::Other { amount, .. } => other = other.saturating_add(amount.0),
            }
        }
        (bitcoin, other, pending)
    }

    fn view_of(&self, bill: &Bill) -> BillView {
        let (received_bitcoin, received_other, pending) = self.bill_totals(bill);
        let received = received_bitcoin.saturating_add(received_other);
        BillView {
            bill: bill.clone(),
            received_bitcoin,
            received_other,
            pending,
            left: bill.amount.0.saturating_sub(received),
        }
    }

    /// CR-246: one bill with what it has received.
    pub fn bill_view(&self, number: u32) -> Option<BillView> {
        self.bills.get(number).map(|b| self.view_of(b))
    }

    /// CR-253: the bills held, the open ones and those that ended in the last
    /// 7 days, oldest first.
    pub fn bill_views(&self) -> Vec<BillView> {
        self.bills.iter().map(|b| self.view_of(b)).collect()
    }

    /// CR-248: the bill a sale is a part of.
    pub fn bill_of_sale(&self, sale: u32) -> Option<u32> {
        self.bills.bill_of_sale(sale)
    }

    /// CR-246: the number the next bill gets.
    pub fn next_bill_number(&self) -> u32 {
        self.bills.next_number()
    }

    /// Write the bills file: the ended bills older than 7 days leave it.
    fn put_bills<S: Store>(
        &mut self,
        store: &mut S,
        bills: Bills,
        now: u64,
    ) -> Result<(), StoreFault<S::Error>> {
        let bills = bills.pruned(now);
        put(store, Slot::Bills, &bills.to_bytes())?;
        self.bills = bills;
        Ok(())
    }

    fn open_bill_ref<E>(&self, number: u32) -> Result<&Bill, BillRefused<E>> {
        let bill = self
            .bills
            .get(number)
            .ok_or(BillRefused::UnknownBill(number))?;
        if bill.is_open() {
            Ok(bill)
        } else {
            Err(BillRefused::NotOpen)
        }
    }

    /// CR-246, CR-253: open a bill or a tab for `amount` minor units of the
    /// shop's currency, with the till's reference when there is one. A bill
    /// is for more than nothing and at most [`MAX_BILL_AMOUNT`]; at most 256
    /// are open.
    ///
    /// The writes, in this order: the bills file with the next number (the
    /// number is stored before it is used), the record "bill opened" and, with
    /// a reference, its record, then the bills file with the bill.
    pub fn open_bill<S: Store>(
        &mut self,
        kind: BillKind,
        amount: Amount,
        reference: Option<&str>,
        now: u64,
        store: &mut S,
    ) -> Result<u32, BillRefused<S::Error>> {
        if amount.0 == 0 || amount.0 > MAX_BILL_AMOUNT {
            return Err(BillRefused::BadAmount);
        }
        if reference.is_some_and(|r| !reference_ok(r)) {
            return Err(BillRefused::BadReference);
        }
        if self.bills.open_count() >= MAX_OPEN_BILLS {
            return Err(BillRefused::TooManyOpen);
        }
        let number = self.bills.next;
        let next = number.checked_add(1).ok_or(BillRefused::TooManyOpen)?;
        let mut bills = self.bills.clone();
        bills.next = next;
        self.put_bills(store, bills, now)
            .map_err(BillRefused::Store)?;
        let currency = Currency::of_shop(self.settings.currency);
        let event = Event::BillOpened {
            number,
            at: now,
            kind,
            amount,
            currency,
        };
        self.write_event(store, &event)
            .map_err(BillRefused::Store)?;
        if let Some(reference) = reference {
            let event = Event::BillReferenced {
                number,
                at: now,
                reference: reference.to_string(),
            };
            self.write_event(store, &event)
                .map_err(BillRefused::Store)?;
        }
        let mut bills = self.bills.clone();
        bills.bills.insert(
            number,
            Bill {
                number,
                kind,
                amount,
                currency,
                opened_at: now,
                reference: reference.map(str::to_string),
                parts: Vec::new(),
                ended: None,
            },
        );
        self.put_bills(store, bills, now)
            .map_err(BillRefused::Store)?;
        Ok(number)
    }

    /// CR-245: add the till's reference to a bill that has none.
    pub fn add_bill_reference<S: Store>(
        &mut self,
        number: u32,
        reference: &str,
        now: u64,
        store: &mut S,
    ) -> Result<(), BillRefused<S::Error>> {
        if !reference_ok(reference) {
            return Err(BillRefused::BadReference);
        }
        let bill = self
            .bills
            .get(number)
            .ok_or(BillRefused::UnknownBill(number))?;
        if bill.reference.is_some() {
            return Err(BillRefused::AlreadyReferenced);
        }
        let event = Event::BillReferenced {
            number,
            at: now,
            reference: reference.to_string(),
        };
        self.write_event(store, &event)
            .map_err(BillRefused::Store)?;
        let mut bills = self.bills.clone();
        if let Some(b) = bills.bills.get_mut(&number) {
            b.reference = Some(reference.to_string());
        }
        self.put_bills(store, bills, now)
            .map_err(BillRefused::Store)
    }

    /// CR-248: the checks that a sale may join a bill, before anything is
    /// written. The sale is open or paid, in the bill's currency, and a part
    /// of no bill.
    fn check_join<E>(&self, bill: &Bill, sale: u32) -> Result<(), BillRefused<E>> {
        if bill.parts.len() >= MAX_BILL_PARTS {
            return Err(BillRefused::TooManyParts);
        }
        let s = self
            .sales
            .get(&sale)
            .ok_or(BillRefused::UnknownSale(sale))?;
        if let Some(n) = self.bills.bill_of_sale(sale) {
            return Err(BillRefused::SaleInABill(n));
        }
        if s.currency() != bill.currency {
            return Err(BillRefused::OtherCurrency);
        }
        if !s.is_live() && s.pos != PosState::Paid {
            return Err(BillRefused::SaleEnded);
        }
        Ok(())
    }

    /// CR-248: join a sale that is open or already paid, such as a paid
    /// request to send (CR-205), to a bill. A sale is a part of at most one
    /// bill, and joining is final. The part's amount is the sale's own amount,
    /// without its tip (CR-189). The bill is settled at once when the part
    /// reaches its amount (CR-249).
    ///
    /// The writes: the record "sale became a part", then the bills file.
    pub fn join_sale<S: Store>(
        &mut self,
        number: u32,
        sale: u32,
        now: u64,
        store: &mut S,
    ) -> Result<(), BillRefused<S::Error>> {
        let bill = self.open_bill_ref(number)?;
        self.check_join(bill, sale)?;
        self.add_sale_part(number, sale, now, store)?;
        self.bills_follow(now, store).map_err(BillRefused::Store)?;
        Ok(())
    }

    fn add_sale_part<S: Store>(
        &mut self,
        number: u32,
        sale: u32,
        now: u64,
        store: &mut S,
    ) -> Result<(), BillRefused<S::Error>> {
        let event = Event::BillPartSale {
            number,
            at: now,
            sale,
        };
        self.write_event(store, &event)
            .map_err(BillRefused::Store)?;
        let mut bills = self.bills.clone();
        if let Some(b) = bills.bills.get_mut(&number) {
            b.parts.push(BillPart::Sale(sale));
        }
        self.put_bills(store, bills, now)
            .map_err(BillRefused::Store)
    }

    /// CR-248: open a sale for the bill and make it a part. The checks of the
    /// bill are made first, so a bill that is full or ended opens no sale; then
    /// the sale opens with every rule of a sale (`extras` may carry the till's
    /// reference or a note, not a request). If the part cannot be written after
    /// the sale is stored, the sale stands and `Err` is returned: its address
    /// must not be shown, and the sale is cancelled or joined by hand.
    #[allow(clippy::too_many_arguments)]
    pub fn open_bill_part<S: Store>(
        &mut self,
        number: u32,
        amount: Amount,
        tip: Amount,
        rate: Rate,
        source: &RateSource,
        facts: OpenFacts,
        by: Option<&Actor>,
        extras: SaleExtras<'_>,
        store: &mut S,
    ) -> Result<Opened, PartRefused<S::Error>> {
        let bill = self.open_bill_ref(number).map_err(PartRefused::Bill)?;
        if bill.parts.len() >= MAX_BILL_PARTS {
            return Err(PartRefused::Bill(BillRefused::TooManyParts));
        }
        let currency = bill.currency;
        let opened = self
            .open_with_extras(
                amount, tip, currency, rate, source, facts, by, extras, store,
            )
            .map_err(PartRefused::Open)?;
        self.add_sale_part(number, opened.index, facts.now, store)
            .map_err(PartRefused::Bill)?;
        Ok(opened)
    }

    /// CR-247: a part paid another way, with what the cashier typed. It
    /// counts at once. The bill is settled when it reaches its amount
    /// (CR-249).
    pub fn add_other_part<S: Store>(
        &mut self,
        number: u32,
        amount: Amount,
        way: PartWay,
        now: u64,
        store: &mut S,
    ) -> Result<(), BillRefused<S::Error>> {
        let bill = self.open_bill_ref(number)?;
        if amount.0 == 0 || amount.0 > MAX_BILL_AMOUNT {
            return Err(BillRefused::BadAmount);
        }
        if bill.parts.len() >= MAX_BILL_PARTS {
            return Err(BillRefused::TooManyParts);
        }
        let event = Event::BillPartPaid {
            number,
            at: now,
            amount,
            way,
        };
        self.write_event(store, &event)
            .map_err(BillRefused::Store)?;
        let mut bills = self.bills.clone();
        if let Some(b) = bills.bills.get_mut(&number) {
            b.parts.push(BillPart::Other { amount, way });
        }
        self.put_bills(store, bills, now)
            .map_err(BillRefused::Store)?;
        self.bills_follow(now, store).map_err(BillRefused::Store)?;
        Ok(())
    }

    /// CR-250: what is left of a bill split between `people` (2 to 20): the
    /// amount each pays, rounded up to a minor unit, the last share taking
    /// what is left over. Nothing is written: each share is opened as a part
    /// when its customer is ready.
    pub fn bill_split<E>(&self, number: u32, people: u8) -> Result<Vec<u64>, BillRefused<E>> {
        let view = self
            .bill_view(number)
            .ok_or(BillRefused::UnknownBill(number))?;
        split_shares(view.left, people).map_err(BillRefused::BadSplit)
    }

    /// CR-251: change a tab's amount while it is open: raise it, or lower it
    /// to no less than what it has received. A bill that is not a tab does not
    /// change.
    pub fn set_tab_amount<S: Store>(
        &mut self,
        number: u32,
        amount: Amount,
        now: u64,
        store: &mut S,
    ) -> Result<(), BillRefused<S::Error>> {
        let bill = self.open_bill_ref(number)?;
        if bill.kind != BillKind::Tab {
            return Err(BillRefused::NotATab);
        }
        if amount.0 == 0 || amount.0 > MAX_BILL_AMOUNT {
            return Err(BillRefused::BadAmount);
        }
        let received = self.view_of(bill).received();
        if amount.0 < received {
            return Err(BillRefused::BelowReceived { received });
        }
        let event = Event::BillAmount {
            number,
            at: now,
            amount,
        };
        self.write_event(store, &event)
            .map_err(BillRefused::Store)?;
        let mut bills = self.bills.clone();
        if let Some(b) = bills.bills.get_mut(&number) {
            b.amount = amount;
        }
        self.put_bills(store, bills, now)
            .map_err(BillRefused::Store)
    }

    /// Write the end of a bill: the record, then the file.
    fn end_bill<S: Store>(
        &mut self,
        number: u32,
        how: BillEnd,
        unpaid: Amount,
        now: u64,
        store: &mut S,
    ) -> Result<(), StoreFault<S::Error>> {
        let event = Event::BillEnded {
            number,
            at: now,
            how,
            unpaid,
        };
        self.write_event(store, &event)?;
        let mut bills = self.bills.clone();
        if let Some(b) = bills.bills.get_mut(&number) {
            b.ended = Some(Ended {
                how,
                at: now,
                unpaid,
            });
        }
        self.put_bills(store, bills, now)
    }

    /// CR-249: settle every open bill (not a tab) whose parts that count have
    /// reached its amount, and return their numbers. The counter calls it
    /// after a sale is judged paid, a part joins or is added; the node may
    /// call it as well, and a bill already settled is not written again. A
    /// failed write stops the call and the next call tries again.
    pub fn bills_follow<S: Store>(
        &mut self,
        now: u64,
        store: &mut S,
    ) -> Result<Vec<u32>, StoreFault<S::Error>> {
        let ready: Vec<u32> = self
            .bills
            .iter()
            .filter(|b| b.is_open() && b.kind == BillKind::Bill)
            .filter(|b| self.view_of(b).left == 0)
            .map(|b| b.number)
            .collect();
        for number in &ready {
            self.end_bill(*number, BillEnd::Settled, Amount(0), now, store)?;
        }
        Ok(ready)
    }

    /// CR-251: the cashier says a tab is finished. It is settled only when it
    /// has received its amount; until then it stays open whatever has arrived.
    pub fn finish_tab<S: Store>(
        &mut self,
        number: u32,
        now: u64,
        store: &mut S,
    ) -> Result<(), BillRefused<S::Error>> {
        let bill = self.open_bill_ref(number)?;
        if bill.kind != BillKind::Tab {
            return Err(BillRefused::NotAFinishableBill);
        }
        let left = self.view_of(bill).left;
        if left > 0 {
            return Err(BillRefused::NotReached { left });
        }
        self.end_bill(number, BillEnd::Settled, Amount(0), now, store)
            .map_err(BillRefused::Store)
    }

    /// CR-252: close a bill that has received less than its amount, with the
    /// manager's PIN (the caller has checked it, CR-84): what was not paid is
    /// written in the record's amount.
    pub fn close_short<S: Store>(
        &mut self,
        number: u32,
        by: &Actor,
        now: u64,
        store: &mut S,
    ) -> Result<Amount, BillRefused<S::Error>> {
        let bill = self.open_bill_ref(number)?;
        if by.role != Role::Manager {
            return Err(BillRefused::NeedsManager);
        }
        let left = self.view_of(bill).left;
        if left == 0 {
            return Err(BillRefused::NothingShort);
        }
        self.end_bill(number, BillEnd::ClosedShort, Amount(left), now, store)
            .map_err(BillRefused::Store)?;
        Ok(Amount(left))
    }

    /// CR-252: cancel a bill with no part that counts and none pending. One
    /// that has received something or has a part pending is not cancelled: its
    /// parts are refunded by the rules of refunds if they are to be.
    pub fn cancel_bill<S: Store>(
        &mut self,
        number: u32,
        now: u64,
        store: &mut S,
    ) -> Result<(), BillRefused<S::Error>> {
        let bill = self.open_bill_ref(number)?;
        let view = self.view_of(bill);
        if view.received() > 0 || view.pending > 0 {
            return Err(BillRefused::HasPart);
        }
        self.end_bill(number, BillEnd::Cancelled, Amount(0), now, store)
            .map_err(BillRefused::Store)
    }
}
