//! Repeats (RQ-24, RQ-25).

use std::collections::BTreeMap;

use crate::book::{RequestBook, MAX_REQUESTS};
use crate::date::{day_of, Day};
use crate::error::{EndRefused, RepeatRefused};
use crate::model::{Repeat, Request, MAX_OPEN, MAX_REPEATS_AT_A_TIME};

/// A request a repeat is due to make (RQ-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueRepeat {
    /// The series.
    pub series: u64,
    /// Which request of the series it is.
    pub index: u32,
    /// Its due date.
    pub due: Day,
    /// The newest request of the series, whose note, amount and pricing it
    /// takes.
    pub template: u64,
}

impl RequestBook {
    /// Whether the owner stopped a series (RQ-25).
    pub fn is_stopped(&self, series: u64) -> bool {
        self.stopped.binary_search(&series).is_ok()
    }

    /// RQ-25: the requests the series are due to make when the owner app is
    /// opened on `today`: for each running series, the next due date less
    /// its lead is today or before. Oldest due date first, at most 12 in all.
    /// The caller takes that many address indexes and calls
    /// [`make_repeat`](Self::make_repeat) for each, in this order.
    pub fn repeats_due(&self, today: Day) -> Vec<DueRepeat> {
        // The newest request of each running series.
        let mut heads: BTreeMap<u64, &Request> = BTreeMap::new();
        for request in &self.requests {
            if let Some(r) = request.repeat {
                if self.is_stopped(r.series) {
                    continue;
                }
                let newer = heads
                    .get(&r.series)
                    .is_none_or(|h| h.repeat.is_some_and(|hr| hr.index < r.index));
                if newer {
                    heads.insert(r.series, request);
                }
            }
        }
        let mut next: Vec<(u64, u64, Repeat)> = heads
            .into_iter()
            .filter_map(|(series, head)| head.repeat.map(|r| (series, head.number, r)))
            .collect();
        let mut out = Vec::new();
        while out.len() < MAX_REPEATS_AT_A_TIME {
            let mut best: Option<(usize, Day)> = None;
            for (at, (_, _, r)) in next.iter().enumerate() {
                let Some(due) = r.every.due(r.first_due, r.index + 1) else {
                    continue;
                };
                if today.0.saturating_add(u32::from(r.lead)) < due.0 {
                    continue;
                }
                if best.is_none_or(|(_, d)| due < d) {
                    best = Some((at, due));
                }
            }
            let Some((at, due)) = best else {
                break;
            };
            let (series, template, r) = &mut next[at];
            r.index += 1;
            out.push(DueRepeat {
                series: *series,
                index: r.index,
                due,
                template: *template,
            });
        }
        out
    }

    /// RQ-25: make one of the requests [`repeats_due`](Self::repeats_due)
    /// named, with the next address index of the desk. The new request has
    /// the next number, the note, amount and pricing of the series and the
    /// due date; it is listed under "Ready to send" until it is shared once.
    pub fn make_repeat(
        &mut self,
        due: &DueRepeat,
        address_index: u32,
        now: u64,
        tz_offset_secs: i32,
    ) -> Result<u64, RepeatRefused> {
        let template = self.request(due.template).ok_or(RepeatRefused::NotDue)?;
        let series = template
            .repeat
            .filter(|r| r.series == due.series)
            .ok_or(RepeatRefused::NotDue)?;
        let newest = self
            .requests
            .iter()
            .filter_map(|r| r.repeat)
            .filter(|r| r.series == due.series)
            .map(|r| r.index)
            .max();
        if self.is_stopped(due.series)
            || newest != Some(due.index.wrapping_sub(1))
            || series.every.due(series.first_due, due.index) != Some(due.due)
        {
            return Err(RepeatRefused::NotDue);
        }
        if self.open_count() >= MAX_OPEN {
            return Err(RepeatRefused::TooManyOpen);
        }
        if self.index_used(address_index) {
            return Err(RepeatRefused::IndexUsed);
        }
        if self.requests.len() >= MAX_REQUESTS {
            return Err(RepeatRefused::BookFull);
        }
        let number = self.next_number;
        let request = Request {
            number,
            note: template.note.clone(),
            amount: template.amount,
            pricing: template.pricing,
            made_at: now,
            made: day_of(now, tz_offset_secs),
            due: due.due,
            address_index,
            repeat: Some(Repeat {
                index: due.index,
                ..series
            }),
            end: None,
            quotes: Vec::new(),
            credits: Vec::new(),
        };
        self.requests.push(request);
        self.next_number += 1;
        Ok(number)
    }

    /// RQ-25: stop the series a request belongs to. The requests already
    /// made stay. Stopping a stopped series changes nothing.
    pub fn stop_repeating(&mut self, number: u64) -> Result<(), EndRefused> {
        let request = self.request(number).ok_or(EndRefused::NoSuchRequest)?;
        let series = request.repeat.ok_or(EndRefused::NoRepeat)?.series;
        if let Err(at) = self.stopped.binary_search(&series) {
            self.stopped.insert(at, series);
        }
        Ok(())
    }
}
