//! The list of requests (RQ-21, RQ-22).

use crate::book::RequestBook;
use crate::date::Day;
use crate::model::{Request, DUE_SOON_DAYS};

/// One row of the list: the request and what is still owed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row<'a> {
    /// The request: its note, number and due date.
    pub request: &'a Request,
    /// What is still owed, in the request's unit.
    pub owed: u64,
}

/// The parts of the list, each with the newest due date last (RQ-21).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListParts<'a> {
    /// RQ-25: made by a repeat and not shared once yet.
    pub ready_to_send: Vec<Row<'a>>,
    /// Past the due day and not paid.
    pub overdue: Vec<Row<'a>>,
    /// Due within 3 days.
    pub due_soon: Vec<Row<'a>>,
    /// The rest.
    pub waiting: Vec<Row<'a>>,
}

impl RequestBook {
    /// RQ-21, RQ-25: the open requests in their parts, for `today` in the
    /// shop's time zone. Within a part the order is by due date, then number.
    pub fn list(&self, today: Day) -> ListParts<'_> {
        let mut parts = ListParts::default();
        for request in self.requests.iter().filter(|r| r.is_open()) {
            let row = Row {
                request,
                owed: request.owed(),
            };
            let part = if request.is_ready_to_send() {
                &mut parts.ready_to_send
            } else if request.due < today {
                &mut parts.overdue
            } else if request.due.0 - today.0 <= DUE_SOON_DAYS {
                &mut parts.due_soon
            } else {
                &mut parts.waiting
            };
            part.push(row);
        }
        for part in [
            &mut parts.ready_to_send,
            &mut parts.overdue,
            &mut parts.due_soon,
            &mut parts.waiting,
        ] {
            part.sort_by_key(|r| (r.request.due, r.request.number));
        }
        parts
    }

    /// RQ-22: how many requests are overdue, for Home.
    pub fn overdue_count(&self, today: Day) -> usize {
        self.list(today).overdue.len()
    }
}
