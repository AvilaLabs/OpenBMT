//! The till's own payments and the match against what arrived (BK-46,
//! BK-47).
//!
//! A fiscal till exports a CSV of its payments. The owner says once, per
//! till, which column holds the reference, the amount, the payment type and
//! the time, and which payment type means bitcoin ([`TillMapping`]).
//! [`read_till_csv`] reads the file strictly and [`Books::match_till`] pairs
//! the lines the till closed as bitcoin with the sales and bills the counter
//! saw paid, by reference. The match changes no record and no total: it is a
//! report. This module has no network, no clock and no file access.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use openbmt_core::say::{Tone, Who, Words};
use openbmt_counter::Currency;

use crate::books::Books;
use crate::date::{Date, MAX_UNIX};
use crate::model::{Bill, BillKey, BillPartNote, Sale, SaleId, SaleKey};
use crate::money::minor_digits;

/// BK-46: the largest file the reader takes, in bytes: 8 MiB.
pub const MAX_TILL_BYTES: usize = 8 * 1024 * 1024;
/// BK-46: the most lines a file has.
pub const MAX_TILL_LINES: usize = 100_000;
/// BK-46: the longest a payment type or a reference is, in bytes.
pub const MAX_TILL_TEXT: usize = 256;
/// The amounts are read to this many decimals and kept as whole millionths.
const AMOUNT_DECIMALS: u32 = 6;

/// Why a mapping is not allowed (BK-46).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingError {
    /// Two of the four columns are the same column.
    SameColumn,
    /// The name of the payment type that means bitcoin is empty, too long
    /// or has a control character.
    BitcoinType,
}

/// BK-46: where each thing is in the till's file, and which payment type
/// means bitcoin. Columns are counted from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillMapping {
    /// The column that holds the reference (the check or receipt number).
    pub reference: u8,
    /// The column that holds the amount.
    pub amount: u8,
    /// The column that holds the payment type.
    pub payment_type: u8,
    /// The column that holds the time.
    pub time: u8,
    /// The payment type that means bitcoin, as the till writes it. Capital
    /// and small letters are the same.
    pub bitcoin_type: String,
    /// The first line of the file names the columns and is not a payment.
    pub has_header: bool,
}

impl TillMapping {
    /// Check the mapping: four different columns and a name for bitcoin.
    pub fn check(&self) -> Result<(), MappingError> {
        let cols = [self.reference, self.amount, self.payment_type, self.time];
        let distinct: BTreeSet<u8> = cols.into_iter().collect();
        if distinct.len() != cols.len() {
            return Err(MappingError::SameColumn);
        }
        let t = self.bitcoin_type.trim();
        if t.is_empty() || t.len() > MAX_TILL_TEXT || t.chars().any(char::is_control) {
            return Err(MappingError::BitcoinType);
        }
        Ok(())
    }

    fn widest(&self) -> usize {
        usize::from(
            self.reference
                .max(self.amount)
                .max(self.payment_type)
                .max(self.time),
        )
    }
}

/// What was wrong with a till's file (BK-46).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TillCsvProblem {
    /// The mapping is not allowed.
    Mapping(MappingError),
    /// The file is more than 8 MiB.
    TooLarge,
    /// The file has more than 100,000 lines.
    TooManyLines,
    /// The line is not UTF-8.
    NotUtf8,
    /// A quote is opened and never closed.
    UnclosedQuote,
    /// Text follows the closing quote of a field.
    TextAfterQuote,
    /// A quote inside a field that does not start with one.
    QuoteInField,
    /// The line is empty.
    EmptyLine,
    /// The line has fewer fields than the mapping reaches.
    TooFewFields,
    /// A field is longer than the reader takes.
    FieldTooLong,
    /// The amount is not a decimal number the reader takes.
    BadAmount,
    /// The time is neither a date nor Unix seconds.
    BadTime,
}

/// BK-46: a file that does not read is refused whole, naming the line (0
/// when it is about the whole file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TillCsvError {
    /// The line, counted from 1; 0 for the whole file.
    pub line: usize,
    /// What was wrong.
    pub problem: TillCsvProblem,
}

impl TillCsvError {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        "BK-46"
    }
}

impl fmt::Display for TillCsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.problem {
            TillCsvProblem::Mapping(MappingError::SameColumn) => {
                "two of the columns are the same column"
            }
            TillCsvProblem::Mapping(MappingError::BitcoinType) => {
                "the name of the payment type for bitcoin is not allowed"
            }
            TillCsvProblem::TooLarge => "the file is more than 8 MiB",
            TillCsvProblem::TooManyLines => "the file has more than 100,000 lines",
            TillCsvProblem::NotUtf8 => "the line is not UTF-8",
            TillCsvProblem::UnclosedQuote => "a quote is opened and never closed",
            TillCsvProblem::TextAfterQuote => "text follows a closing quote",
            TillCsvProblem::QuoteInField => {
                "a quote is inside a field that does not start with one"
            }
            TillCsvProblem::EmptyLine => "the line is empty",
            TillCsvProblem::TooFewFields => "the line has fewer fields than the mapping reaches",
            TillCsvProblem::FieldTooLong => "a field is longer than the reader takes",
            TillCsvProblem::BadAmount => "the amount is not a decimal number",
            TillCsvProblem::BadTime => "the time is not a date or Unix seconds",
        };
        if self.line == 0 {
            write!(f, "BK-46: {what}")
        } else {
            write!(f, "BK-46: line {}: {what}", self.line)
        }
    }
}

impl std::error::Error for TillCsvError {}

/// One payment line of the till's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillLine {
    /// The line of the file it starts on, counted from 1.
    pub line: usize,
    /// The reference; none when the field is empty.
    pub reference: Option<String>,
    /// The amount in millionths of the currency's unit (a negative amount
    /// is a refund).
    pub amount_micro: i64,
    /// The payment type, as the till wrote it.
    pub payment_type: String,
    /// The day of the line: the date as the till wrote it, or the date of
    /// Unix seconds at the shop's offset.
    pub day: Date,
}

impl TillLine {
    /// Whether the line was closed as bitcoin under this mapping.
    pub fn is_bitcoin(&self, mapping: &TillMapping) -> bool {
        self.payment_type
            .trim()
            .eq_ignore_ascii_case(mapping.bitcoin_type.trim())
    }
}

/// RFC 4180 records, each with the line it starts on.
fn records(text: &str) -> Result<Vec<(usize, Vec<String>)>, TillCsvError> {
    let err = |line, problem| TillCsvError { line, problem };
    let mut out: Vec<(usize, Vec<String>)> = Vec::new();
    let mut chars = text.chars().peekable();
    let mut line = 1usize;
    loop {
        let start = line;
        let mut fields: Vec<String> = Vec::new();
        loop {
            let mut field = String::new();
            if chars.peek() == Some(&'"') {
                chars.next();
                loop {
                    match chars.next() {
                        None => return Err(err(start, TillCsvProblem::UnclosedQuote)),
                        Some('"') => {
                            if chars.peek() == Some(&'"') {
                                chars.next();
                                field.push('"');
                            } else {
                                break;
                            }
                        }
                        Some(c) => {
                            if c == '\n' {
                                line += 1;
                            }
                            field.push(c);
                        }
                    }
                }
                match chars.peek() {
                    None | Some(',' | '\n' | '\r') => {}
                    Some(_) => return Err(err(line, TillCsvProblem::TextAfterQuote)),
                }
            } else {
                while let Some(&c) = chars.peek() {
                    if matches!(c, ',' | '\n' | '\r') {
                        break;
                    }
                    if c == '"' {
                        return Err(err(line, TillCsvProblem::QuoteInField));
                    }
                    field.push(c);
                    chars.next();
                }
            }
            if field.len() > MAX_TILL_TEXT * 4 {
                return Err(err(start, TillCsvProblem::FieldTooLong));
            }
            fields.push(field);
            match chars.next() {
                Some(',') => continue,
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    line += 1;
                    break;
                }
                Some('\n') => {
                    line += 1;
                    break;
                }
                _ => {
                    // The end of the file after the last field.
                    out.push((start, fields));
                    return Ok(out);
                }
            }
        }
        if fields.len() == 1 && fields.first().is_some_and(String::is_empty) {
            if chars.peek().is_none() {
                // One empty line at the very end is the file's last newline
                // written twice; the rest are refused.
                return Ok(out);
            }
            return Err(err(start, TillCsvProblem::EmptyLine));
        }
        out.push((start, fields));
        if chars.peek().is_none() {
            return Ok(out);
        }
    }
}

/// An amount as whole millionths: an optional `-`, digits and up to six
/// decimals after a point.
fn parse_amount(text: &str) -> Option<i64> {
    let t = text.trim();
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let (whole, frac) = match body.split_once('.') {
        Some((w, f)) => (w, f),
        None => (body, ""),
    };
    if whole.is_empty()
        || whole.len() > 12
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || frac.len() > AMOUNT_DECIMALS as usize
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut micro: i64 = whole.parse::<i64>().ok()?.checked_mul(1_000_000)?;
    let mut scale = 100_000i64;
    for b in frac.bytes() {
        micro = micro.checked_add(i64::from(b - b'0') * scale)?;
        scale /= 10;
    }
    Some(if negative { -micro } else { micro })
}

/// A time as a day: `YYYY-MM-DD` at the start (a time of day after it is
/// not read), or Unix seconds at the shop's offset.
fn parse_day(text: &str, offset_minutes: i32) -> Option<Date> {
    let t = text.trim();
    if t.bytes().all(|b| b.is_ascii_digit()) && !t.is_empty() {
        let secs: u64 = t.parse().ok()?;
        return Date::from_unix_offset(secs.min(MAX_UNIX + 1), offset_minutes);
    }
    let b = t.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    if b.len() < 10
        || !(0..4).all(digit)
        || b.get(4) != Some(&b'-')
        || !(5..7).all(digit)
        || b.get(7) != Some(&b'-')
        || !(8..10).all(digit)
        || b.get(10).is_some_and(|c| !matches!(c, b'T' | b' '))
    {
        return None;
    }
    let year: i32 = t.get(0..4)?.parse().ok()?;
    let month: u8 = t.get(5..7)?.parse().ok()?;
    let day: u8 = t.get(8..10)?.parse().ok()?;
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    (year >= 1970 && (1..=max).contains(&day)).then_some(Date { year, month, day })
}

/// BK-46: read the till's payments from the bytes of its CSV file. At most
/// 8 MiB and 100,000 lines, RFC 4180 (BK-20) and UTF-8; a file that does not
/// read is refused whole, naming the line. `offset_minutes` is the shop's
/// offset from UTC (BK-16), used for a time given in Unix seconds.
pub fn read_till_csv(
    bytes: &[u8],
    mapping: &TillMapping,
    offset_minutes: i32,
) -> Result<Vec<TillLine>, TillCsvError> {
    let whole = |problem| TillCsvError { line: 0, problem };
    mapping
        .check()
        .map_err(|e| whole(TillCsvProblem::Mapping(e)))?;
    if bytes.len() > MAX_TILL_BYTES {
        return Err(whole(TillCsvProblem::TooLarge));
    }
    let lines = bytes.iter().filter(|b| **b == b'\n').count() + 1;
    // A final newline does not start a line.
    let lines = if bytes.last() == Some(&b'\n') {
        lines - 1
    } else {
        lines
    };
    if lines > MAX_TILL_LINES {
        return Err(TillCsvError {
            line: MAX_TILL_LINES + 1,
            problem: TillCsvProblem::TooManyLines,
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|e| {
        let good = bytes.get(..e.valid_up_to()).unwrap_or_default();
        TillCsvError {
            line: good.iter().filter(|b| **b == b'\n').count() + 1,
            problem: TillCsvProblem::NotUtf8,
        }
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    for (n, (line, fields)) in records(text)?.into_iter().enumerate() {
        if n == 0 && mapping.has_header {
            continue;
        }
        let err = |problem| TillCsvError { line, problem };
        if fields.len() <= mapping.widest() {
            return Err(err(TillCsvProblem::TooFewFields));
        }
        let field = |c: u8| fields.get(usize::from(c)).map_or("", |s| s.trim());
        let reference = field(mapping.reference);
        let payment_type = field(mapping.payment_type);
        if reference.len() > MAX_TILL_TEXT || payment_type.len() > MAX_TILL_TEXT {
            return Err(err(TillCsvProblem::FieldTooLong));
        }
        out.push(TillLine {
            line,
            reference: (!reference.is_empty()).then(|| reference.to_string()),
            amount_micro: parse_amount(field(mapping.amount))
                .ok_or_else(|| err(TillCsvProblem::BadAmount))?,
            payment_type: payment_type.to_string(),
            day: parse_day(field(mapping.time), offset_minutes)
                .ok_or_else(|| err(TillCsvProblem::BadTime))?,
        });
    }
    Ok(out)
}

/// A sale or a bill of the books that the match looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TillItemKey {
    /// A sale.
    Sale(SaleKey),
    /// A bill or a tab.
    Bill(BillKey),
}

/// A paid sale or bill, as the match sees it (BK-47).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillItem {
    /// Which.
    pub key: TillItemKey,
    /// The opaque sale ID, for a sale (BK-07).
    pub sale_id: Option<SaleId>,
    /// The reference the counter was given.
    pub reference: Option<String>,
    /// What bitcoin paid, in minor units: the sale's own amount, or what a
    /// bill received in bitcoin.
    pub amount_minor: u64,
    /// The currency.
    pub currency: Currency,
    /// The day it opened at the shop's offset.
    pub day: Date,
}

/// BK-47: a reference that is on both sides, with the lines and the items
/// that carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillPair {
    /// The reference.
    pub reference: String,
    /// The lines of the file, by their line numbers.
    pub lines: Vec<usize>,
    /// What the till's lines came to, in millionths of the currency's unit.
    pub till_micro: i64,
    /// The paid sales and bills.
    pub items: Vec<TillItem>,
    /// What they came to, in minor units.
    pub books_minor: u64,
}

/// BK-47: the report of the match. Nothing in the books changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TillMatch {
    /// Same reference, amounts equal to the minor unit.
    pub matched: Vec<TillPair>,
    /// Same reference, different amount.
    pub amount_differs: Vec<TillPair>,
    /// Lines the till closed as bitcoin with no paid sale or bill behind
    /// them.
    pub till_without_payment: Vec<TillLine>,
    /// Paid sales and bills whose reference is in no line of the file.
    pub payment_not_in_till: Vec<TillItem>,
    /// Paid sales and bills whose reference is in the file only on lines of
    /// another payment type: the till says they were not paid in bitcoin.
    pub paid_on_other_type: Vec<TillItem>,
    /// Bitcoin lines with no reference, to be matched by hand.
    pub till_without_reference: Vec<TillLine>,
    /// Paid sales and bills with no reference, to be matched by hand.
    pub payment_without_reference: Vec<TillItem>,
}

impl TillMatch {
    /// Whether every list is empty.
    pub fn is_empty(&self) -> bool {
        self.matched.is_empty()
            && self.amount_differs.is_empty()
            && self.till_without_payment.is_empty()
            && self.payment_not_in_till.is_empty()
            && self.paid_on_other_type.is_empty()
            && self.till_without_reference.is_empty()
            && self.payment_without_reference.is_empty()
    }
}

fn sale_item(sale: &Sale, offset_minutes: i32) -> Option<TillItem> {
    Some(TillItem {
        key: TillItemKey::Sale(sale.key),
        sale_id: Some(sale.id),
        reference: sale.till_reference.clone(),
        amount_minor: sale.amount_minor,
        currency: sale.currency,
        day: Date::from_unix_offset(sale.opened_at, offset_minutes)?,
    })
}

fn bill_item(books: &Books, bill: &Bill, offset_minutes: i32) -> Option<TillItem> {
    let (bitcoin, _) = books.bill_received(bill).ok()?;
    Some(TillItem {
        key: TillItemKey::Bill(bill.key),
        sale_id: None,
        reference: bill.reference.clone(),
        amount_minor: bitcoin,
        currency: bill.currency,
        day: Date::from_unix_offset(bill.opened_at, offset_minutes)?,
    })
}

/// The minor units a sum of millionths comes to, when it is a whole number
/// of them.
fn micro_to_minor(micro: i64, currency: Currency) -> Option<i64> {
    let digits = minor_digits(currency).min(AMOUNT_DECIMALS);
    let unit = 10i64.pow(AMOUNT_DECIMALS - digits);
    (micro % unit == 0).then_some(micro / unit)
}

impl Books {
    /// BK-47: match the lines of the till's file against the books. It takes
    /// the lines whose payment type is bitcoin, and the paid sales and bills
    /// of the days the file covers, and pairs them by reference: lines that
    /// share a reference are added together, as are the sales and bills that
    /// share one. A sale that is a part of a bill that has a reference is
    /// counted with the bill. `offset_minutes` is the shop's offset from UTC
    /// (BK-16), for the day of a sale. The books are not changed.
    pub fn match_till(
        &self,
        lines: &[TillLine],
        mapping: &TillMapping,
        offset_minutes: i32,
    ) -> TillMatch {
        let days: BTreeSet<Date> = lines.iter().map(|l| l.day).collect();
        let in_bills: BTreeSet<SaleKey> = self
            .bills
            .iter()
            .filter(|b| b.reference.is_some())
            .flat_map(|b| b.parts.iter())
            .filter_map(|p| match p {
                BillPartNote::Sale(key) => Some(*key),
                BillPartNote::Other { .. } => None,
            })
            .collect();
        let mut items: Vec<TillItem> = Vec::new();
        for sale in &self.sales {
            if sale.state.is_paid() && !in_bills.contains(&sale.key) {
                items.extend(sale_item(sale, offset_minutes));
            }
        }
        for bill in &self.bills {
            // A bill or a tab: what bitcoin paid of it.
            items.extend(bill_item(self, bill, offset_minutes).filter(|i| i.amount_minor > 0));
        }
        items.retain(|i| days.contains(&i.day));

        let mut report = TillMatch::default();
        let bitcoin: Vec<&TillLine> = lines.iter().filter(|l| l.is_bitcoin(mapping)).collect();
        let mut by_ref: BTreeMap<&str, Vec<&TillLine>> = BTreeMap::new();
        for l in &bitcoin {
            match &l.reference {
                Some(r) => by_ref.entry(r).or_default().push(l),
                None => report.till_without_reference.push((*l).clone()),
            }
        }
        let any_ref: BTreeSet<&str> = lines
            .iter()
            .filter_map(|l| l.reference.as_deref())
            .collect();
        let mut items_by_ref: BTreeMap<&str, Vec<&TillItem>> = BTreeMap::new();
        for item in &items {
            match &item.reference {
                Some(r) => items_by_ref.entry(r).or_default().push(item),
                None => report.payment_without_reference.push(item.clone()),
            }
        }
        for (reference, till) in &by_ref {
            let Some(paid) = items_by_ref.get(reference) else {
                report
                    .till_without_payment
                    .extend(till.iter().map(|l| (*l).clone()));
                continue;
            };
            let till_micro: i64 = till.iter().map(|l| l.amount_micro).sum();
            let books_minor = paid
                .iter()
                .fold(0u64, |s, i| s.saturating_add(i.amount_minor));
            let one_currency = paid
                .first()
                .map(|i| i.currency)
                .filter(|c| paid.iter().all(|i| i.currency == *c));
            let equal = one_currency.is_some_and(|c| {
                micro_to_minor(till_micro, c)
                    .is_some_and(|m| i128::from(m) == i128::from(books_minor))
            });
            let pair = TillPair {
                reference: (*reference).to_string(),
                lines: till.iter().map(|l| l.line).collect(),
                till_micro,
                items: paid.iter().map(|i| (*i).clone()).collect(),
                books_minor,
            };
            if equal {
                report.matched.push(pair);
            } else {
                report.amount_differs.push(pair);
            }
        }
        for (reference, paid) in &items_by_ref {
            if by_ref.contains_key(reference) {
                continue;
            }
            let list = if any_ref.contains(reference) {
                &mut report.paid_on_other_type
            } else {
                &mut report.payment_not_in_till
            };
            list.extend(paid.iter().map(|i| (*i).clone()));
        }
        report
    }
}

/// BK-48: the placeholder of the words of a refusal of the till's file,
/// until the words are written under `docs/SCREEN-RULES.md`.
fn needed(placeholder: &'static str) -> Words {
    Words::new(
        Tone::Stop,
        placeholder,
        format!("{placeholder}_WHAT."),
        Who::Owner,
        format!("{placeholder}_NEXT."),
    )
    .code("BK-46")
}

/// BK-46: the placeholder constants of the words of the till's file.
pub const WORDS_NEEDED_BK_46_MAP: &str = "WORDS_NEEDED_BK_46_MAP";
/// BK-46: the file is more than 8 MiB.
pub const WORDS_NEEDED_BK_46_SIZE: &str = "WORDS_NEEDED_BK_46_SIZE";
/// BK-46: the file has more than 100,000 lines.
pub const WORDS_NEEDED_BK_46_LINES: &str = "WORDS_NEEDED_BK_46_LINES";
/// BK-46: a line is not UTF-8.
pub const WORDS_NEEDED_BK_46_UTF8: &str = "WORDS_NEEDED_BK_46_UTF8";
/// BK-46: a line breaks RFC 4180 (a quote or an empty line).
pub const WORDS_NEEDED_BK_46_CSV: &str = "WORDS_NEEDED_BK_46_CSV";
/// BK-46: a line has fewer fields than the mapping reaches.
pub const WORDS_NEEDED_BK_46_FIELDS: &str = "WORDS_NEEDED_BK_46_FIELDS";
/// BK-46: an amount is not a decimal number.
pub const WORDS_NEEDED_BK_46_AMOUNT: &str = "WORDS_NEEDED_BK_46_AMOUNT";
/// BK-46: a time is not a date or Unix seconds.
pub const WORDS_NEEDED_BK_46_TIME: &str = "WORDS_NEEDED_BK_46_TIME";

impl TillCsvError {
    /// What the owner is told when the till's file is refused: the
    /// placeholder of the cause, with the line in the details (BK-48).
    pub fn words(&self) -> Words {
        let placeholder = match self.problem {
            TillCsvProblem::Mapping(_) => WORDS_NEEDED_BK_46_MAP,
            TillCsvProblem::TooLarge => WORDS_NEEDED_BK_46_SIZE,
            TillCsvProblem::TooManyLines => WORDS_NEEDED_BK_46_LINES,
            TillCsvProblem::NotUtf8 => WORDS_NEEDED_BK_46_UTF8,
            TillCsvProblem::UnclosedQuote
            | TillCsvProblem::TextAfterQuote
            | TillCsvProblem::QuoteInField
            | TillCsvProblem::EmptyLine
            | TillCsvProblem::FieldTooLong => WORDS_NEEDED_BK_46_CSV,
            TillCsvProblem::TooFewFields => WORDS_NEEDED_BK_46_FIELDS,
            TillCsvProblem::BadAmount => WORDS_NEEDED_BK_46_AMOUNT,
            TillCsvProblem::BadTime => WORDS_NEEDED_BK_46_TIME,
        };
        needed(placeholder).detail(self.to_string())
    }
}

/// SC-12: the words of every refusal of the till's file, one for each
/// placeholder. Kept apart from [`crate::say::all_words`] until the words are
/// written.
pub fn till_words() -> Vec<(String, Words)> {
    use TillCsvProblem as P;
    [
        P::Mapping(MappingError::SameColumn),
        P::Mapping(MappingError::BitcoinType),
        P::TooLarge,
        P::TooManyLines,
        P::NotUtf8,
        P::UnclosedQuote,
        P::TextAfterQuote,
        P::QuoteInField,
        P::EmptyLine,
        P::TooFewFields,
        P::FieldTooLong,
        P::BadAmount,
        P::BadTime,
    ]
    .into_iter()
    .map(|problem| {
        let e = TillCsvError { line: 3, problem };
        (format!("TillCsvError::{problem:?}"), e.words())
    })
    .collect()
}
