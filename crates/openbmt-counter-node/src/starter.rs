//! The starter line at the counter (`docs/COUNTER-RULES.md` section 26,
//! CR-272 to CR-276).
//!
//! A counter paired with the starter wallet and not switched takes the
//! owner phone's starter line (`OBM_`, `docs/LINK-RULES.md` LK-79) and keeps
//! the last one in [`crate::dirstore::STARTER_LINE_FILE`] (CR-272). From it
//! the counter works out whether "New sale" offers bitcoin
//! ([`stop_decision`], CR-273), and from the payments in the lines it has
//! taken it keeps the cards of Home and the day's list of Today in
//! [`crate::dirstore::STARTER_CARDS_FILE`] (CR-274). A switch drops both
//! (CR-275).

use std::fmt;

use openbmt_core::starter_line::{LinePayment, StarterLine};
use openbmt_core::wire::{self, DecodeError, Reader};

/// The magic of the file of the starter payments (`OBM"`), version 1:
/// the count (u16 LE), then per payment the transaction ID (32), the sats
/// (u64 LE), the time (u64 LE), the flag of the owner phone's own payment
/// and the flag of a payment seen at the counter, in the order of the
/// transaction IDs, none twice.
pub const MAGIC_STARTER_CARDS: [u8; 4] = *b"OBM\x1c";

/// Most payments the counter keeps (CR-274). One more drops the oldest that
/// was seen, or else the oldest.
pub const MAX_CARDS: usize = 256;

/// Bytes of one payment in the file.
const CARD_BYTES: usize = 32 + 8 + 8 + 1 + 1;

/// CR-273: what the last line taken says to "New sale".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopDecision {
    /// Offer bitcoin and say nothing: there is no line, or the line does not
    /// ask for a stop that applies.
    Open,
    /// The last line is fresh, says stop and holds at least the limit:
    /// "New sale" offers no bitcoin and says why (SS-38).
    Stopped,
    /// The last line said stop but is older than the heartbeat age limit:
    /// bitcoin is offered as before and Home says that the limit cannot be
    /// checked.
    Unchecked,
}

/// CR-273: the stop decision, from the last line taken (if any), the time
/// now and the heartbeat age limit of the settings in force, in seconds. A
/// line is fresh while its time is within the limit of the time now, either
/// side, so that a line signed by a clock far from this one is not taken for
/// a fresh one.
pub fn stop_decision(line: Option<&StarterLine>, now: u64, max_age_secs: u64) -> StopDecision {
    let Some(line) = line.filter(|l| l.stop) else {
        return StopDecision::Open;
    };
    if now.abs_diff(line.time) > max_age_secs {
        return StopDecision::Unchecked;
    }
    if line.held_sats >= line.limit_sats {
        StopDecision::Stopped
    } else {
        StopDecision::Open
    }
}

/// One payment out of the starter wallet as the counter keeps it (CR-274).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterCard {
    /// The payment as the line gave it.
    pub payment: LinePayment,
    /// Someone at the counter tapped that they have seen it.
    pub seen: bool,
}

/// What the counter keeps of the starter payments, in the order of their
/// transaction IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cards(Vec<StarterCard>);

impl Cards {
    /// The payments kept, newest first.
    pub fn newest_first(&self) -> Vec<StarterCard> {
        let mut out = self.0.clone();
        out.sort_by(|a, b| {
            b.payment
                .time
                .cmp(&a.payment.time)
                .then(a.payment.txid.cmp(&b.payment.txid))
        });
        out
    }

    /// How many are kept.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether none is kept.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// CR-274: add the payments of a line that are not kept yet, unseen.
    /// Returns whether anything was added.
    pub fn add(&mut self, payments: &[LinePayment]) -> bool {
        let mut added = false;
        for p in payments {
            if let Err(at) = self.0.binary_search_by(|c| c.payment.txid.cmp(&p.txid)) {
                self.0.insert(
                    at,
                    StarterCard {
                        payment: *p,
                        seen: false,
                    },
                );
                added = true;
            }
        }
        while self.0.len() > MAX_CARDS {
            let oldest = |seen: bool| {
                self.0
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.seen == seen)
                    .min_by_key(|(_, c)| (c.payment.time, c.payment.txid))
                    .map(|(i, _)| i)
            };
            match oldest(true).or_else(|| oldest(false)) {
                Some(i) => {
                    self.0.remove(i);
                }
                None => break,
            }
        }
        added
    }

    /// CR-274: mark one payment seen. Returns whether it was kept and
    /// unseen.
    pub fn mark_seen(&mut self, txid: &[u8; 32]) -> bool {
        match self.0.iter_mut().find(|c| c.payment.txid == *txid) {
            Some(c) if !c.seen => {
                c.seen = true;
                true
            }
            _ => false,
        }
    }

    /// The payments not seen yet, newest first: the cards of Home.
    pub fn unseen(&self) -> Vec<StarterCard> {
        self.newest_first()
            .into_iter()
            .filter(|c| !c.seen)
            .collect()
    }

    /// CR-274: the payments with a time in `from..to`, newest first: the
    /// day's list of Today.
    pub fn between(&self, from: u64, to: u64) -> Vec<StarterCard> {
        self.newest_first()
            .into_iter()
            .filter(|c| (from..to).contains(&c.payment.time))
            .collect()
    }

    /// The wire form of the file.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_STARTER_CARDS, 2 + self.0.len() * CARD_BYTES);
        out.extend_from_slice(
            &u16::try_from(self.0.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        for c in &self.0 {
            out.extend_from_slice(&c.payment.txid);
            out.extend_from_slice(&c.payment.sats.to_le_bytes());
            out.extend_from_slice(&c.payment.time.to_le_bytes());
            out.push(u8::from(c.payment.by_owner));
            out.push(u8::from(c.seen));
        }
        out
    }

    /// Read the file strictly: exact length, at most [`MAX_CARDS`], in the
    /// order of the transaction IDs and none twice, flags 0 or 1.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_STARTER_CARDS)?;
        let n = usize::from(r.u16_le()?);
        if n > MAX_CARDS {
            return Err(DecodeError::TooLarge);
        }
        let mut cards: Vec<StarterCard> = Vec::with_capacity(n);
        for _ in 0..n {
            let txid: [u8; 32] = r.array()?;
            if cards.last().is_some_and(|c| c.payment.txid >= txid) {
                return Err(DecodeError::NonCanonical(
                    "the payments are not in order, or one is twice",
                ));
            }
            cards.push(StarterCard {
                payment: LinePayment {
                    txid,
                    sats: r.u64_le()?,
                    time: r.u64_le()?,
                    by_owner: r.flag()?,
                },
                seen: r.flag()?,
            });
        }
        r.finish()?;
        Ok(Self(cards))
    }
}

/// Why a starter line was not taken, or a payment not marked seen (CR-272,
/// CR-274). Each names its rule; nothing changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarterRefused {
    /// CR-272: the frame is not a starter line.
    Frame(DecodeError),
    /// CR-272: the counter is not paired with a starter wallet, or has
    /// switched.
    NotStarter,
    /// CR-272: the signature is not the pinned owner root's.
    NotFromOwner,
    /// CR-272: the number is not above the last one taken.
    Old {
        /// The number of the last line taken.
        last: u64,
        /// The number of this one.
        got: u64,
    },
    /// CR-272, CR-274: a file could not be written.
    NotWritten(String),
    /// CR-274: the payment is not kept, or was seen already.
    NoSuchPayment,
}

impl StarterRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NoSuchPayment => "CR-274",
            _ => "CR-272",
        }
    }
}

impl fmt::Display for StarterRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Frame(e) => write!(f, "what came as a starter line is not one: {e}"),
            Self::NotStarter => write!(
                f,
                "this counter is not paired with a starter wallet, or has switched"
            ),
            Self::NotFromOwner => write!(f, "the starter line is not signed by the owner root"),
            Self::Old { last, got } => write!(
                f,
                "the starter line's number {got} is not above the last one taken, {last}"
            ),
            Self::NotWritten(e) => write!(f, "the starter line was not saved: {e}"),
            Self::NoSuchPayment => write!(f, "no unseen payment of that transaction is kept"),
        }
    }
}

impl std::error::Error for StarterRefused {}
