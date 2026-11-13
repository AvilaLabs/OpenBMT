//! WO-10, WO-20, WO-30: what the store answers to the owner phone's three
//! calls, read strictly. The store's answers are JSON of one object; any
//! other shape is refused, and a number is a whole number.

use openbmt_json::{Limits, Value};

use super::check::StoreOrder;
use super::WebOrderRefusal;

/// The most bytes of a store's answer to a range or a notice that are read.
const MAX_SMALL_BYTES: usize = 4096;

/// The store's answer to a request for orders (WO-20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdersReply {
    /// The orders made since the number asked.
    pub orders: Vec<StoreOrder>,
    /// How many addresses the store has left (WO-11).
    pub left: u64,
}

impl OrdersReply {
    /// Reads `{"orders":[..],"left":12}`. A reply with no `left` is refused:
    /// the owner phone cannot tell when the store runs low without it.
    pub fn parse(text: &[u8]) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::Orders;
        let orders = StoreOrder::parse_list(text)?;
        let limits = Limits {
            max_bytes: text.len().max(1),
            max_depth: 4,
            max_values: 4096,
        };
        let doc = openbmt_json::parse(text, &limits).map_err(|_| bad)?;
        let left = doc.get("left").and_then(Value::as_u64).ok_or(bad)?;
        Ok(Self { orders, left })
    }
}

/// What the store says of a range it was sent (WO-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeResult {
    /// The store holds the range now.
    Accepted,
    /// The store held it already.
    Stale,
    /// The range starts after where the store's last one ended.
    Gap,
    /// The range starts before where the store's last one ended.
    Overlap,
    /// An address of the range is one the store has.
    Duplicate,
    /// The range is signed for another pairing.
    WrongPairing,
}

impl RangeResult {
    /// Whether the store holds the range.
    pub fn held(self) -> bool {
        matches!(self, Self::Accepted | Self::Stale)
    }

    /// The byte kept in the owner phone's file.
    pub fn byte(self) -> u8 {
        match self {
            Self::Accepted => 1,
            Self::Stale => 2,
            Self::Gap => 3,
            Self::Overlap => 4,
            Self::Duplicate => 5,
            Self::WrongPairing => 6,
        }
    }

    /// The result of a byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Accepted),
            2 => Some(Self::Stale),
            3 => Some(Self::Gap),
            4 => Some(Self::Overlap),
            5 => Some(Self::Duplicate),
            6 => Some(Self::WrongPairing),
            _ => None,
        }
    }
}

/// The store's answer to a range: `{"result":"accepted","left":100}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeReply {
    /// What the store did.
    pub result: RangeResult,
    /// The addresses the store has left.
    pub left: u64,
}

fn small(text: &[u8]) -> Result<Value, WebOrderRefusal> {
    let limits = Limits {
        max_bytes: MAX_SMALL_BYTES,
        max_depth: 2,
        max_values: 16,
    };
    openbmt_json::parse(text, &limits).map_err(|_| WebOrderRefusal::Range)
}

impl RangeReply {
    /// Reads the store's answer to a range.
    pub fn parse(text: &[u8]) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::Range;
        let doc = small(text)?;
        let result = match doc.get("result").and_then(Value::as_str).ok_or(bad)? {
            "accepted" => RangeResult::Accepted,
            "stale" => RangeResult::Stale,
            "gap" => RangeResult::Gap,
            "overlap" => RangeResult::Overlap,
            "duplicate" => RangeResult::Duplicate,
            "wrong_pairing" => RangeResult::WrongPairing,
            _ => return Err(bad),
        };
        let left = doc.get("left").and_then(Value::as_u64).ok_or(bad)?;
        Ok(Self { result, left })
    }
}

/// What the store did with a notice (WO-31, WO-32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    /// The order was marked paid.
    Complete,
    /// A note was added and the order left on hold.
    Note,
    /// Nothing changed: the order was paid already.
    Nothing,
    /// The store refused the notice and logged it.
    Refuse,
}

impl NoticeAction {
    /// Reads `{"action":"complete"}`.
    pub fn parse(text: &[u8]) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::Notice;
        let limits = Limits {
            max_bytes: MAX_SMALL_BYTES,
            max_depth: 2,
            max_values: 16,
        };
        let doc = openbmt_json::parse(text, &limits).map_err(|_| bad)?;
        match doc.get("action").and_then(Value::as_str).ok_or(bad)? {
            "complete" => Ok(Self::Complete),
            "note" => Ok(Self::Note),
            "none" => Ok(Self::Nothing),
            "refuse" => Ok(Self::Refuse),
            _ => Err(bad),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WO-20: the reply of the plugin's orders route reads, with the number of
    /// addresses it has left, and a reply without it or of another shape does
    /// not.
    #[test]
    fn wo_20_the_orders_reply_reads_strictly() {
        let text = br#"{"orders":[{"order":7,"address":"bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080","sats":5000,"quoted_at":1800000000,"total_minor":250,"currency":"USD"}],"left":42}"#;
        let reply = OrdersReply::parse(text).unwrap();
        assert_eq!((reply.orders.len(), reply.left), (1, 42));
        assert_eq!(reply.orders[0].order, 7);
        for bad in [
            &br#"{"orders":[]}"#[..],
            br#"{"orders":[],"left":-1}"#,
            br#"{"orders":[],"left":"3"}"#,
            br#"[]"#,
            b"",
        ] {
            assert_eq!(OrdersReply::parse(bad), Err(WebOrderRefusal::Orders));
        }
    }

    /// WO-10: each answer the plugin gives to a range reads, and a word it
    /// does not give does not.
    #[test]
    fn wo_10_the_range_reply_reads_strictly() {
        for (word, result) in [
            ("accepted", RangeResult::Accepted),
            ("stale", RangeResult::Stale),
            ("gap", RangeResult::Gap),
            ("overlap", RangeResult::Overlap),
            ("duplicate", RangeResult::Duplicate),
            ("wrong_pairing", RangeResult::WrongPairing),
        ] {
            let text = format!("{{\"result\":\"{word}\",\"left\":100}}");
            let reply = RangeReply::parse(text.as_bytes()).unwrap();
            assert_eq!((reply.result, reply.left), (result, 100));
            assert_eq!(RangeResult::from_byte(result.byte()), Some(result));
        }
        assert!(RangeResult::Accepted.held() && RangeResult::Stale.held());
        assert!(!RangeResult::Gap.held());
        assert_eq!(RangeResult::from_byte(0), None);
        for bad in [
            &br#"{"result":"ok","left":1}"#[..],
            br#"{"result":"gap"}"#,
            b"{}",
        ] {
            assert_eq!(RangeReply::parse(bad), Err(WebOrderRefusal::Range));
        }
    }

    /// WO-31, WO-32: each action the plugin answers a notice with reads.
    #[test]
    fn wo_31_the_notice_reply_reads_strictly() {
        for (word, action) in [
            ("complete", NoticeAction::Complete),
            ("note", NoticeAction::Note),
            ("none", NoticeAction::Nothing),
            ("refuse", NoticeAction::Refuse),
        ] {
            let text = format!("{{\"action\":\"{word}\"}}");
            assert_eq!(NoticeAction::parse(text.as_bytes()), Ok(action));
        }
        assert_eq!(
            NoticeAction::parse(br#"{"action":"cancel"}"#),
            Err(WebOrderRefusal::Notice)
        );
        assert_eq!(NoticeAction::parse(b"{}"), Err(WebOrderRefusal::Notice));
    }
}
