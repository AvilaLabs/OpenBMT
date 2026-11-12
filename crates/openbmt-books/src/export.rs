//! The two CSV exports (BK-17 to BK-22).

use std::collections::{BTreeMap, BTreeSet};

use openbmt_counter::Currency;

use crate::books::Books;
use crate::csv::{push_row, Cell};
use crate::date::{iso_time, Date, YearMonth};
use crate::error::{hex_id, ExportError};
use crate::model::{
    bill_state_text, way_text, Bill, BillPartNote, Counting, DisposalKind, Sale, SaleId, SaleKey,
    Settlement,
};
use crate::money::{format_minor, format_signed};
use crate::value::{UnknownField, Value};
use openbmt_counter::BillKind;

/// BK-19: the sentence at the top of the full ledger (DESIGN §10).
pub const LEDGER_WARNING: &str = "This file reveals the shop's payments on the blockchain. \
Share it only with people you trust with that.";

/// BK-19: the acknowledgment the caller must pass to get the full ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealsPayments {
    /// The caller has shown the warning and the person accepted it.
    Acknowledged,
}

/// One line of the accounting export.
struct Line {
    at: u64,
    description: String,
    negative: bool,
    minor: u64,
    currency: Currency,
}

fn date_of(at: u64) -> Result<Date, ExportError> {
    Date::from_unix(at).ok_or(ExportError::TimeOutOfRange { at })
}

fn id_of(key: &SaleKey) -> SaleId {
    SaleId::for_sale(key)
}

impl Books {
    /// BK-17, BK-18, BK-21, BK-22: the accounting export, one CSV per UTC
    /// calendar month, oldest first. Lines hold the opaque sale ID only.
    pub fn accounting_export(&self) -> Result<Vec<(YearMonth, String)>, ExportError> {
        let mut lines: Vec<Line> = Vec::new();
        // BK-43: a sale that is a part of a bill with a reference and has none
        // of its own carries the bill's, since it is the till's own number.
        let mut bill_refs: BTreeMap<SaleKey, &str> = BTreeMap::new();
        for bill in &self.bills {
            if let Some(r) = &bill.reference {
                for part in &bill.parts {
                    if let BillPartNote::Sale(key) = part {
                        bill_refs.insert(*key, r);
                    }
                }
            }
        }
        for sale in &self.sales {
            if sale.counting() != Counting::Counted {
                continue;
            }
            let Some(at) = sale.first_funds_at else {
                continue;
            };
            // BK-38: the line is what the sale was paid, the amount and the
            // tip together; the tip is on a line of its own in the ledger.
            // BK-43: the reference follows the opaque sale ID.
            let description = match sale
                .till_reference
                .as_deref()
                .or_else(|| bill_refs.get(&sale.key).copied())
            {
                Some(r) => format!("Sale {} ref {r}", sale.id),
                None => format!("Sale {}", sale.id),
            };
            lines.push(Line {
                at,
                description,
                negative: false,
                minor: sale.due_minor().ok_or(ExportError::Overflow)?,
                currency: sale.currency,
            });
        }
        for (position, d) in self.disposals.iter().enumerate() {
            let minor = d
                .minor_value
                .ok_or(ExportError::DisposalValueUnknown { position })?;
            let description = match d.kind {
                DisposalKind::Refund { sale } => format!("Refund of sale {}", id_of(&sale)),
                DisposalKind::CashOut => "Bitcoin cash-out".to_string(),
                DisposalKind::Fee => "Network fee paid in bitcoin".to_string(),
            };
            lines.push(Line {
                at: d.at,
                description,
                negative: true,
                minor,
                currency: d.currency,
            });
        }
        for r in &self.returns {
            let currency = self
                .sale(&r.sale)
                .map(|s| s.currency)
                .ok_or(ExportError::SaleMissing { sale: r.sale })?;
            lines.push(Line {
                at: r.at,
                description: format!("Cash return for sale {}", id_of(&r.sale)),
                negative: true,
                minor: r.minor,
                currency,
            });
        }
        let currencies: BTreeSet<Currency> = lines.iter().map(|l| l.currency).collect();
        if currencies.len() > 1 {
            return Err(ExportError::MixedCurrencies);
        }
        lines.sort_by_key(|l| l.at);
        let mut files: Vec<(YearMonth, String)> = Vec::new();
        for line in &lines {
            let date = date_of(line.at)?;
            let month = date.year_month();
            if files.last().map(|(m, _)| *m) != Some(month) {
                let mut out = String::new();
                push_row(
                    &mut out,
                    &[
                        Cell::text("Date"),
                        Cell::text("Description"),
                        Cell::text("Amount"),
                    ],
                );
                files.push((month, out));
            }
            if let Some((_, out)) = files.last_mut() {
                push_row(
                    out,
                    &[
                        Cell::Number(date.to_string()),
                        Cell::text(line.description.clone()),
                        Cell::Number(format_signed(line.negative, line.minor, line.currency)),
                    ],
                );
            }
        }
        Ok(files)
    }

    /// BK-19 to BK-22: the full ledger, with a warning row first. It cannot
    /// be made without the acknowledgment.
    pub fn full_ledger(&self, _ack: RevealsPayments) -> Result<String, ExportError> {
        let mut out = String::new();
        push_row(&mut out, &[Cell::text(format!("# {LEDGER_WARNING}"))]);
        let marks: Vec<String> = self
            .high_water
            .iter()
            .map(|(c, hw)| {
                let v = hw.map_or("none".to_string(), |i| i.to_string());
                format!("{}={v}", hex_id(c))
            })
            .collect();
        let marks = if marks.is_empty() {
            "none".to_string()
        } else {
            marks.join(" ")
        };
        push_row(
            &mut out,
            &[Cell::text(format!(
                "# High-water mark (highest sale index per counter): {marks}"
            ))],
        );
        push_row(&mut out, &LEDGER_COLUMNS.map(Cell::text));
        for sale in &self.sales {
            push_row(&mut out, &sale_row(sale)?);
        }
        // BK-44: one line per bill after the sales.
        for bill in &self.bills {
            push_row(&mut out, &self.bill_row(bill)?);
        }
        for d in &self.disposals {
            let (kind, sale) = match d.kind {
                DisposalKind::Refund { sale } => ("refund", Some(sale)),
                DisposalKind::CashOut => ("cash-out", None),
                DisposalKind::Fee => ("fee", None),
            };
            let mut unknown = Vec::new();
            let value = match d.minor_value {
                Some(v) => Cell::Number(format_minor(v, d.currency)),
                None => {
                    unknown.push(UnknownField::DisposalValue);
                    Cell::text("unknown")
                }
            };
            let basis = match d.basis {
                Some(b) => Cell::Number(format_minor(b.minor, d.currency)),
                None => {
                    unknown.push(UnknownField::DisposalBasis);
                    Cell::text("unknown")
                }
            };
            let mut row = blank_row();
            set(&mut row, 0, Cell::text(kind));
            set_sale_ref(&mut row, sale.as_ref());
            set(&mut row, 6, time_cell(d.at)?);
            set(&mut row, 8, Cell::text(d.currency.to_string()));
            set(&mut row, 9, value);
            set(&mut row, 11, Cell::Number(d.sats.to_string()));
            set(&mut row, 15, basis);
            set(&mut row, 19, explain(&unknown));
            push_row(&mut out, &row);
        }
        for r in &self.returns {
            let currency = self
                .sale(&r.sale)
                .map(|s| s.currency)
                .ok_or(ExportError::SaleMissing { sale: r.sale })?;
            let mut row = blank_row();
            set(&mut row, 0, Cell::text("cash-return"));
            set_sale_ref(&mut row, Some(&r.sale));
            set(&mut row, 6, time_cell(r.at)?);
            set(&mut row, 8, Cell::text(currency.to_string()));
            set(
                &mut row,
                9,
                Cell::Number(format_signed(true, r.minor, currency)),
            );
            push_row(&mut out, &row);
        }
        Ok(out)
    }
}

const LEDGER_COLUMNS: [&str; 37] = [
    "Type",
    "Sale ID",
    "Counter",
    "Sale index",
    "State",
    "Counted",
    "Time (UTC)",
    "Opened (UTC)",
    "Currency",
    "Amount",
    "Sales tax",
    "Sats",
    "Quote rate (minor per BTC)",
    "Rate source",
    "Value at first seen",
    "Basis",
    "Txid",
    "Address",
    "Note",
    "Unknown",
    "Staff",
    "First seen (UTC)",
    "Last quote sats",
    "Last quote rate (minor per BTC)",
    "Last rate source",
    "Payment mode",
    "Payment hash",
    "Lightning amount (msat)",
    "Lightning settled (UTC)",
    "Tips",
    "Sale note",
    "Till reference",
    "Bill number",
    "Bill received in bitcoin",
    "Bill received another way",
    "Bill unpaid when closed short",
    "Bill sale IDs",
];

fn blank_row() -> Vec<Cell> {
    vec![Cell::Empty; LEDGER_COLUMNS.len()]
}

fn set(row: &mut [Cell], at: usize, cell: Cell) {
    if let Some(slot) = row.get_mut(at) {
        *slot = cell;
    }
}

fn set_sale_ref(row: &mut [Cell], sale: Option<&SaleKey>) {
    if let Some(key) = sale {
        set(row, 1, Cell::text(id_of(key).to_string()));
        set(row, 2, Cell::text(hex_id(&key.counter)));
        set(row, 3, Cell::Number(key.index.to_string()));
    }
}

fn time_cell(at: u64) -> Result<Cell, ExportError> {
    iso_time(at)
        .map(Cell::Number)
        .ok_or(ExportError::TimeOutOfRange { at })
}

/// A cell that says what each unknown is, why, and what would supply it.
fn explain(fields: &[UnknownField]) -> Cell {
    if fields.is_empty() {
        return Cell::Empty;
    }
    let parts: Vec<String> = fields
        .iter()
        .map(|f| {
            let u = f.explain();
            format!("{}: {}. Next: {}", f.name(), u.why, u.next_step)
        })
        .collect();
    Cell::text(parts.join(" | "))
}

fn value_cell<T>(
    v: &Value<T>,
    show: impl Fn(&T) -> String,
    unknown: &mut Vec<UnknownField>,
) -> Cell {
    match v {
        Value::Known(x) => Cell::Number(show(x)),
        Value::Unknown(f) => {
            unknown.push(*f);
            Cell::text("unknown")
        }
    }
}

fn sale_row(sale: &Sale) -> Result<Vec<Cell>, ExportError> {
    let mut unknown = Vec::new();
    let mut row = blank_row();
    set(&mut row, 0, Cell::text("sale"));
    set_sale_ref(&mut row, Some(&sale.key));
    set(&mut row, 4, Cell::text(sale.state.text()));
    set(&mut row, 5, Cell::text(sale.counting().text()));
    if let Some(at) = sale.first_funds_at {
        set(&mut row, 6, time_cell(at)?);
    }
    set(&mut row, 7, time_cell(sale.opened_at)?);
    set(&mut row, 8, Cell::text(sale.currency.to_string()));
    set(
        &mut row,
        9,
        Cell::Number(format_minor(sale.amount_minor, sale.currency)),
    );
    set(
        &mut row,
        10,
        value_cell(
            &sale.sales_tax_minor,
            |v| format_minor(*v, sale.currency),
            &mut unknown,
        ),
    );
    set(
        &mut row,
        11,
        value_cell(&sale.sats_received, u64::to_string, &mut unknown),
    );
    set(
        &mut row,
        12,
        Cell::Number(sale.quote_rate_minor_per_btc.to_string()),
    );
    set(
        &mut row,
        13,
        value_cell(&sale.rate_source, |s| s.label().to_string(), &mut unknown),
    );
    set(
        &mut row,
        23,
        Cell::Number(sale.last_quote_rate_minor_per_btc.to_string()),
    );
    set(&mut row, 22, Cell::Number(sale.last_quote_sats.to_string()));
    set(
        &mut row,
        24,
        value_cell(
            &sale.last_rate_source,
            |s| s.label().to_string(),
            &mut unknown,
        ),
    );
    if sale.funds_seen {
        set(
            &mut row,
            14,
            value_cell(
                &sale.value_at_first_seen_minor,
                |v| format_minor(*v, sale.currency),
                &mut unknown,
            ),
        );
        if let Some(at) = sale.first_seen_at {
            set(&mut row, 21, time_cell(at)?);
        }
        set(&mut row, 16, txid_cell(sale, &mut unknown));
        set(
            &mut row,
            17,
            value_cell(&sale.address, String::clone, &mut unknown),
        );
    }
    set(&mut row, 18, note_cell(sale));
    set(&mut row, 19, explain(&unknown));
    set(&mut row, 25, Cell::text(sale.payment_mode.text()));
    if let Some(p) = sale.payment_mode.lightning() {
        set(&mut row, 26, Cell::Number(hex32(&p.payment_hash)));
        set(&mut row, 27, Cell::Number(p.amount_msat.to_string()));
        if let Some(at) = p.settled_at {
            set(&mut row, 28, time_cell(at)?);
        }
    }
    if let Some(tip) = &sale.tip {
        set(
            &mut row,
            29,
            Cell::Number(format_minor(tip.minor, sale.currency)),
        );
    }
    if let Some(note) = &sale.note {
        set(&mut row, 30, Cell::text(note.clone()));
    }
    if let Some(reference) = &sale.till_reference {
        set(&mut row, 31, Cell::text(reference.clone()));
    }
    let names: Vec<&str> = sale.overrides.iter().map(|o| o.name.as_str()).collect();
    if !names.is_empty() {
        set(&mut row, 20, Cell::text(names.join("; ")));
    }
    Ok(row)
}

impl Books {
    /// BK-44: the line of one bill in the full ledger: its number, state,
    /// amount, reference, what it received in bitcoin, what it received
    /// another way (the cashier's word) and what was left unpaid when it was
    /// closed short, and the opaque IDs of its sales.
    fn bill_row(&self, bill: &Bill) -> Result<Vec<Cell>, ExportError> {
        let (bitcoin, other) = self
            .bill_received(bill)
            .map_err(|_| ExportError::Overflow)?;
        let currency = bill.currency;
        let mut row = blank_row();
        set(
            &mut row,
            0,
            Cell::text(match bill.kind {
                BillKind::Bill => "bill",
                BillKind::Tab => "tab",
            }),
        );
        set(&mut row, 2, Cell::text(hex_id(&bill.key.counter)));
        set(&mut row, 4, Cell::text(bill_state_text(bill)));
        if let Some(e) = bill.ended {
            set(&mut row, 6, time_cell(e.at)?);
        }
        set(&mut row, 7, time_cell(bill.opened_at)?);
        set(&mut row, 8, Cell::text(currency.to_string()));
        set(
            &mut row,
            9,
            Cell::Number(format_minor(bill.amount_minor, currency)),
        );
        let others: Vec<String> = bill
            .parts
            .iter()
            .filter_map(|p| match p {
                BillPartNote::Other { minor, way } => Some(format!(
                    "{} {}",
                    format_minor(*minor, currency),
                    way_text(*way)
                )),
                BillPartNote::Sale(_) => None,
            })
            .collect();
        if !others.is_empty() {
            set(
                &mut row,
                18,
                Cell::text(format!(
                    "paid another way by the cashier's word: {}",
                    others.join("; ")
                )),
            );
        }
        if let Some(reference) = &bill.reference {
            set(&mut row, 31, Cell::text(reference.clone()));
        }
        set(&mut row, 32, Cell::Number(bill.key.number.to_string()));
        set(&mut row, 33, Cell::Number(format_minor(bitcoin, currency)));
        set(&mut row, 34, Cell::Number(format_minor(other, currency)));
        if let Some(e) = bill.ended.filter(|e| e.unpaid_minor > 0) {
            set(
                &mut row,
                35,
                Cell::Number(format_minor(e.unpaid_minor, currency)),
            );
        }
        let ids: Vec<String> = bill
            .parts
            .iter()
            .filter_map(|p| match p {
                BillPartNote::Sale(key) => Some(id_of(key).to_string()),
                BillPartNote::Other { .. } => None,
            })
            .collect();
        if !ids.is_empty() {
            set(&mut row, 36, Cell::text(ids.join(" ")));
        }
        Ok(row)
    }
}

/// The transaction IDs the records carry for a sale: those of the final
/// record, then the late payments' that are not among them. It is unknown,
/// and says why, when the sale has a payment of its own and no final record
/// names it.
fn txid_cell(sale: &Sale, unknown: &mut Vec<UnknownField>) -> Cell {
    let mut ids: Vec<String> = Vec::new();
    match &sale.payments {
        Value::Known(list) => ids.extend(list.payments.iter().map(|p| hex32(&p.txid))),
        Value::Unknown(f) => {
            if sale.late_payments.is_empty() || sale.own_payment {
                unknown.push(*f);
            }
        }
    }
    for p in &sale.late_payments {
        let id = hex32(&p.txid);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() && sale.payment_mode.lightning().is_some() {
        // BK-32: a Lightning payment has no transaction.
        Cell::Empty
    } else if ids.is_empty() {
        Cell::text("unknown")
    } else {
        Cell::text(ids.join(" "))
    }
}

fn hex32(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn note_cell(sale: &Sale) -> Cell {
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = sale.at_risk_sats {
        parts.push(format!("accepted at once with {n} sats at risk"));
    }
    if let Some(p) = sale.payment_mode.lightning() {
        parts.push(format!(
            "paid over Lightning, {} msat, final at once",
            p.amount_msat
        ));
    } else if let Value::Known(list) = &sale.payments {
        let n = list.payments.len();
        let more = if list.more { " and more not named" } else { "" };
        let s = if n == 1 { "" } else { "s" };
        parts.push(format!("the final record names {n} payment{s}{more}"));
    }
    for p in &sale.late_payments {
        parts.push(format!("late payment of {} sats", p.sats));
    }
    match sale.settlement() {
        Some(Settlement::Overpaid { surplus_sats }) => {
            parts.push(format!(
                "overpaid: {surplus_sats} sats owed to the customer"
            ));
        }
        Some(Settlement::BelowLastQuote { short_sats }) => {
            parts.push(format!("below the last quote by {short_sats} sats"));
        }
        None => {}
    }
    if sale.dust_sats > 0 {
        parts.push(format!(
            "{} sats arrived below the dust line",
            sale.dust_sats
        ));
    }
    for o in &sale.overrides {
        parts.push(format!("{} override for {} sats", o.role.text(), o.sats));
    }
    match sale.pos {
        crate::model::PosMark::NotRecorded => {}
        crate::model::PosMark::Paid => parts.push("the POS was told paid".to_string()),
        crate::model::PosMark::Unpaid => parts.push("the POS was told not paid".to_string()),
    }
    if parts.is_empty() {
        Cell::Empty
    } else {
        Cell::text(parts.join("; "))
    }
}
