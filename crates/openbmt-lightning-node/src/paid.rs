//! The paid events, kept until the caller acknowledges them (SL-21).
//!
//! A sale is paid on `Event::PaymentClaimed`, after the claim is on both
//! copies. The event is then written here, before the caller is told, and
//! stays until [`PaidLog::ack`]. A start gives every one that is left again,
//! with the same payment hash, so the caller treats a repeat as the same
//! payment.
//!
//! Written the way the manager is, the counter's copy at once and the second
//! device's as soon as it can be reached (SL-24), so that the library's loop
//! never waits for a device that is away (SL-02, SL-23). One blob for each
//! payment hash.
//!
//! An acknowledged event is not deleted at once but marked, and kept for a
//! while: after a crash the library gives `Event::PaymentClaimed` again for a
//! claim it had not finished with, and that repeat must not bring back a sale
//! the caller has already acknowledged. The marks are dropped on a start once
//! they are older than [`ACKED_KEPT_SECS`].

use crate::store::Background;
use lightning::io;
use lightning::util::persist::KVStore;
use std::sync::Arc;

/// Where the paid events are kept.
pub const PRIMARY: &str = "paid_events";
/// How long the mark of an acknowledged event is kept, in seconds: thirty
/// days, far longer than the library can owe a repeat of an event.
pub const ACKED_KEPT_SECS: u64 = 30 * 24 * 3600;
const VERSION: u8 = 3;
/// The second version, which had no funding: read with none.
const VERSION_2: u8 = 2;
/// The first version, which had no skimmed fee: read with a skim of 0.
const VERSION_1: u8 = 1;
const LEN_1: usize = 1 + 8 + 8 + 1 + 8 + 1 + 8;
const LEN_2: usize = LEN_1 + 8;
/// The most fundings a record keeps (SL-40).
pub const MAX_FUNDINGS: usize = 8;

/// A paid sale as it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidRecord {
    /// The payment hash of the invoice.
    pub payment_hash: [u8; 32],
    /// What arrived, in millisatoshis.
    pub amount_msat: u64,
    /// What the provider skimmed off this payment, in millisatoshis: 0 when
    /// no channel was opened for it.
    pub skimmed_msat: u64,
    /// What the payer sent, if the library knew it.
    pub sender_intended_msat: Option<u64>,
    /// When the claim was seen, in seconds since the epoch. A repeat carries
    /// the time of the first.
    pub received_unix: u64,
    /// The funding transaction of every channel the payment came through
    /// whose funding had not the shop's confirmations when it was claimed
    /// (SL-40), once each, at most [`MAX_FUNDINGS`].
    pub unconfirmed_fundings: Vec<[u8; 32]>,
    /// The payment came through more such channels than [`MAX_FUNDINGS`]:
    /// only the first are listed, and it is never final at once. Only with
    /// [`MAX_FUNDINGS`] listed.
    pub more_fundings: bool,
    /// An HTLC of the payment came through a channel that was no longer
    /// listed at the claim, and its funding is not known (SL-27): the payment
    /// is one through a lost funding. Kept as the second bit of the flag
    /// after the count of fundings.
    pub lost_unknown: bool,
}

pub(crate) fn hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn unhex(name: &str) -> Option<[u8; 32]> {
    if name.len() != 64 || !name.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&name[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

pub(crate) fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// What is kept for a hash: the event, and when it was acknowledged if it was.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kept {
    record: PaidRecord,
    acked_unix: Option<u64>,
}

fn encode(k: &Kept) -> Vec<u8> {
    let r = &k.record;
    let mut out = Vec::with_capacity(LEN_2 + 2 + 32 * r.unconfirmed_fundings.len());
    out.push(VERSION);
    out.extend_from_slice(&r.amount_msat.to_be_bytes());
    out.extend_from_slice(&r.received_unix.to_be_bytes());
    out.push(r.sender_intended_msat.is_some() as u8);
    out.extend_from_slice(&r.sender_intended_msat.unwrap_or(0).to_be_bytes());
    out.push(k.acked_unix.is_some() as u8);
    out.extend_from_slice(&k.acked_unix.unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&r.skimmed_msat.to_be_bytes());
    out.push(r.unconfirmed_fundings.len() as u8);
    out.push(r.more_fundings as u8 | (r.lost_unknown as u8) << 1);
    for txid in &r.unconfirmed_fundings {
        out.extend_from_slice(txid);
    }
    out
}

fn decode(hash: [u8; 32], bytes: &[u8]) -> Result<Kept, io::Error> {
    // Version 3 ends with the count of fundings, a flag and the fundings, so
    // its length is told by the count.
    let n = bytes.get(LEN_2).map_or(0, |&n| usize::from(n));
    let ok = match bytes.first() {
        Some(&VERSION) => bytes.len() == LEN_2 + 2 + 32 * n,
        Some(&VERSION_2) => bytes.len() == LEN_2,
        Some(&VERSION_1) => bytes.len() == LEN_1,
        _ => false,
    };
    if !ok {
        return Err(invalid("paid event"));
    }
    // The lengths are checked above, so every field is there.
    let fields = (|| {
        Some((
            be_u64(bytes, 1)?,
            be_u64(bytes, 9)?,
            *bytes.get(17)?,
            be_u64(bytes, 18)?,
            *bytes.get(26)?,
            be_u64(bytes, 27)?,
        ))
    })();
    let Some((amount, received, sender_flag, sender, acked_flag, acked)) = fields else {
        return Err(invalid("paid event"));
    };
    // A flag is 0 or 1, and with 0 its eight bytes are zero, so that the
    // bytes are the ones `encode` makes.
    if sender_flag > 1 || acked_flag > 1 {
        return Err(invalid("paid event: flag"));
    }
    if (sender_flag == 0 && sender != 0) || (acked_flag == 0 && acked != 0) {
        return Err(invalid("paid event: unused bytes"));
    }
    let skimmed = be_u64(bytes, LEN_1).unwrap_or(0);
    // The fundings: a count of at most [`MAX_FUNDINGS`], a flag whose first
    // bit is set only for a full list and whose second says a funding is lost, and the fundings, each once.
    let (fundings, flags) = match bytes
        .get(LEN_2..)
        .filter(|_| bytes.first() == Some(&VERSION))
    {
        None => (Vec::new(), 0),
        Some(b) => {
            // The length was checked against the count: no bytes are left over.
            let (Some(&flag), Some(rest)) = (b.get(1), b.get(2..)) else {
                return Err(invalid("paid event"));
            };
            let list: Vec<[u8; 32]> = rest.as_chunks::<32>().0.to_vec();
            if list.len() > MAX_FUNDINGS {
                return Err(invalid("paid event: too many fundings"));
            }
            if flag > 3 || (flag & 1 == 1 && list.len() != MAX_FUNDINGS) {
                return Err(invalid("paid event: flag"));
            }
            if list
                .iter()
                .enumerate()
                .any(|(i, t)| list.iter().take(i).any(|u| u == t))
            {
                return Err(invalid("paid event: a funding twice"));
            }
            (list, flag)
        }
    };
    Ok(Kept {
        record: PaidRecord {
            payment_hash: hash,
            amount_msat: amount,
            skimmed_msat: skimmed,
            received_unix: received,
            sender_intended_msat: (sender_flag == 1).then_some(sender),
            unconfirmed_fundings: fundings,
            more_fundings: flags & 1 == 1,
            lost_unknown: flags & 2 == 2,
        },
        acked_unix: (acked_flag == 1).then_some(acked),
    })
}

/// The eight bytes at `at` as a number, if they are there.
fn be_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    let eight: [u8; 8] = bytes.get(at..end)?.try_into().ok()?;
    Some(u64::from_be_bytes(eight))
}

/// The event read from `bytes` and written again, for the fuzz target: the
/// result must be the input.
#[cfg(feature = "testing")]
pub fn reencode_for_test(hash: [u8; 32], bytes: &[u8]) -> Option<Vec<u8>> {
    let k = decode(hash, bytes).ok()?;
    let mut out = encode(&k);
    // An older version is the front of the newer one, and what it lacks was
    // read as nothing.
    let version = *bytes.first()?;
    if version != VERSION {
        out.truncate(bytes.len());
        *out.first_mut()? = version;
    }
    Some(out)
}

/// The paid events not yet acknowledged.
pub struct PaidLog {
    store: Arc<Background>,
}

impl PaidLog {
    /// A log over the node's background store.
    pub fn new(store: Arc<Background>) -> Self {
        PaidLog { store }
    }

    async fn read(&self, hash: &[u8; 32]) -> Result<Option<Kept>, io::Error> {
        match KVStore::read(&*self.store, PRIMARY, "", &hex(hash)).await {
            Ok(bytes) => Ok(Some(decode(*hash, &bytes)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Keeps a paid event, and returns the one to give the caller: the one
    /// kept already if there is one (so the time stays that of the first
    /// claim), or `None` if the caller has acknowledged it, so that a repeat
    /// of the library's event after a crash does not bring the sale back.
    pub async fn record(&self, r: PaidRecord) -> Result<Option<PaidRecord>, io::Error> {
        if let Some(kept) = self.read(&r.payment_hash).await? {
            return Ok(kept.acked_unix.is_none().then_some(kept.record));
        }
        let hash = r.payment_hash;
        let kept = Kept {
            record: r,
            acked_unix: None,
        };
        KVStore::write(&*self.store, PRIMARY, "", &hex(&hash), encode(&kept)).await?;
        Ok(Some(kept.record))
    }

    /// Every paid event not acknowledged, oldest first. The marks of events
    /// acknowledged more than [`ACKED_KEPT_SECS`] before `now` are dropped.
    pub async fn pending(&self, now: u64) -> Result<Vec<PaidRecord>, io::Error> {
        let mut out = Vec::new();
        for name in KVStore::list(&*self.store, PRIMARY, "").await? {
            let Some(hash) = unhex(&name) else { continue };
            let Some(kept) = self.read(&hash).await? else {
                continue;
            };
            match kept.acked_unix {
                None => out.push(kept.record),
                Some(at) if at.saturating_add(ACKED_KEPT_SECS) <= now => {
                    KVStore::remove(&*self.store, PRIMARY, "", &name, false).await?;
                }
                Some(_) => {}
            }
        }
        out.sort_by_key(|r| (r.received_unix, r.payment_hash));
        Ok(out)
    }

    /// The caller has the event: it is not given again, not after a restart
    /// and not for a repeat of the library's event. An event that is not kept
    /// is not an error.
    pub async fn ack(&self, payment_hash: &[u8; 32], now: u64) -> Result<(), io::Error> {
        let Some(kept) = self.read(payment_hash).await? else {
            return Ok(());
        };
        if kept.acked_unix.is_some() {
            return Ok(());
        }
        let kept = Kept {
            acked_unix: Some(now),
            ..kept
        };
        KVStore::write(&*self.store, PRIMARY, "", &hex(payment_hash), encode(&kept)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::SecondDevice;
    use crate::seal::open;
    use crate::store::device_name;
    use crate::testutil::{disks, Disks};
    use std::time::Duration;

    /// A fresh log over the same disks, with the flusher running: a start.
    fn start(d: &Disks) -> (PaidLog, tokio::task::JoinHandle<()>) {
        let (background, flusher) = crate::testutil::start_background(d);
        (PaidLog::new(background), flusher)
    }

    fn rec(n: u8, at: u64) -> PaidRecord {
        PaidRecord {
            payment_hash: [n; 32],
            amount_msat: 49_000_000,
            skimmed_msat: 1_000_000,
            sender_intended_msat: Some(50_000_000),
            received_unix: at,
            unconfirmed_fundings: Vec::new(),
            more_fundings: false,
            lost_unknown: false,
        }
    }

    const NOW: u64 = 1_000;

    #[tokio::test]
    async fn sl_21_a_paid_event_comes_again_after_a_restart_until_it_is_acked() {
        let d = disks("paid-restart");
        let (log, flusher) = start(&d);
        log.record(rec(1, 100)).await.unwrap();
        log.record(rec(2, 50)).await.unwrap();
        // Not acknowledged: stopped (here, dropped) and started again.
        flusher.abort();
        drop(log);
        let (log, flusher) = start(&d);
        let again = log.pending(NOW).await.unwrap();
        assert_eq!(again, vec![rec(2, 50), rec(1, 100)]);
        // The same hash, the same amount and time: the same payment.
        assert_eq!(again[1].payment_hash, [1; 32]);
        // Acknowledged: not given again, and the other still is.
        log.ack(&[1; 32], NOW).await.unwrap();
        flusher.abort();
        drop(log);
        let (log, _flusher) = start(&d);
        assert_eq!(log.pending(NOW).await.unwrap(), vec![rec(2, 50)]);
        log.ack(&[2; 32], NOW).await.unwrap();
        // An ack twice, or of what is not kept, is not an error.
        log.ack(&[2; 32], NOW).await.unwrap();
        log.ack(&[9; 32], NOW).await.unwrap();
        assert!(log.pending(NOW).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn sl_21_a_repeated_claim_keeps_the_first_time() {
        let d = disks("paid-repeat");
        let (log, _f) = start(&d);
        let first = log.record(rec(3, 100)).await.unwrap().unwrap();
        let second = log.record(rec(3, 999)).await.unwrap().unwrap();
        assert_eq!(first, second);
        assert_eq!(second.received_unix, 100);
        assert_eq!(log.pending(NOW).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn sl_21_the_librarys_repeat_of_a_claim_after_the_ack_does_not_bring_the_sale_back() {
        let d = disks("paid-replay");
        let (log, _f) = start(&d);
        log.record(rec(5, 100)).await.unwrap();
        log.ack(&[5; 32], NOW).await.unwrap();
        // The library gives PaymentClaimed again after a crash: not delivered, not kept.
        assert_eq!(log.record(rec(5, 500)).await.unwrap(), None);
        assert!(log.pending(NOW).await.unwrap().is_empty());
        // The mark is dropped long after.
        let later = NOW + ACKED_KEPT_SECS;
        assert!(log.pending(later).await.unwrap().is_empty());
        assert!(!d
            .dir
            .join("local")
            .join(PRIMARY)
            .join(hex(&[5; 32]))
            .exists());
    }

    #[tokio::test]
    async fn sl_24_a_paid_event_is_on_the_counter_at_once_and_on_the_device_when_it_is_back() {
        let d = disks("paid-lazy");
        let (log, _f) = start(&d);
        d.device.set_away(true);
        // Does not wait for the device (SL-23).
        tokio::time::timeout(Duration::from_secs(1), log.record(rec(4, 1)))
            .await
            .unwrap()
            .unwrap();
        let key = hex(&[4; 32]);
        assert!(d.dir.join("local").join(PRIMARY).join(&key).exists());
        d.device.set_away(false);
        let name = device_name(PRIMARY, "", &key);
        let mut sealed = None;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            sealed = d.device.read(&name).await.unwrap();
            if sealed.is_some() {
                break;
            }
        }
        let plain = open(&d.key, &name, &sealed.unwrap()).unwrap();
        assert_eq!(decode([4; 32], &plain).unwrap().record, rec(4, 1));
    }

    #[test]
    fn sl_21_the_names_are_the_hash_in_hex_and_a_damaged_blob_is_an_error() {
        let h = [0xab; 32];
        assert_eq!(unhex(&hex(&h)), Some(h));
        assert_eq!(unhex("zz"), None);
        assert_eq!(unhex(&"g".repeat(64)), None);
        assert!(decode(h, &[]).is_err());
        let none = Kept {
            record: PaidRecord {
                sender_intended_msat: None,
                ..rec(1, 5)
            },
            acked_unix: Some(9),
        };
        assert_eq!(decode([1; 32], &encode(&none)).unwrap(), none);
        // The first version has no skim: it reads as none.
        let mut old = encode(&none);
        old.truncate(LEN_1);
        old[0] = VERSION_1;
        let read = decode([1; 32], &old).unwrap();
        assert_eq!(read.record.skimmed_msat, 0);
        assert_eq!(read.record.amount_msat, none.record.amount_msat);
        assert!(decode([1; 32], &old[..LEN_1 - 1]).is_err());
    }

    /// A flag other than 0 or 1, and bytes that are not zero behind a flag of
    /// 0, are refused, so that what is read writes back to the same bytes.
    #[test]
    fn sl_21_a_flag_that_is_not_0_or_1_and_unused_bytes_are_refused() {
        let kept = Kept {
            record: rec(1, 5),
            acked_unix: Some(9),
        };
        let good = encode(&kept);
        assert_eq!(decode([1; 32], &good).unwrap(), kept);
        for at in [17, 26] {
            for flag in [2u8, 255] {
                let mut b = good.clone();
                b[at] = flag;
                assert!(decode([1; 32], &b).is_err());
            }
        }
        let mut sender = encode(&Kept {
            record: PaidRecord {
                sender_intended_msat: None,
                ..rec(1, 5)
            },
            acked_unix: Some(9),
        });
        sender[20] = 1;
        assert!(decode([1; 32], &sender).is_err());
        let mut acked = encode(&Kept {
            record: rec(1, 5),
            acked_unix: None,
        });
        acked[30] = 1;
        assert!(decode([1; 32], &acked).is_err());
    }

    /// What reads writes back to the same bytes, in each of the four ways the
    /// flags can be, and a version 1 blob is read and written as version 2.
    #[test]
    fn sl_21_an_event_that_reads_writes_back_to_the_same_bytes() {
        for sender in [None, Some(7u64)] {
            for acked_unix in [None, Some(9u64)] {
                let kept = Kept {
                    record: PaidRecord {
                        sender_intended_msat: sender,
                        ..rec(1, 5)
                    },
                    acked_unix,
                };
                let bytes = encode(&kept);
                assert_eq!(encode(&decode([1; 32], &bytes).unwrap()), bytes);
            }
        }
        let mut old = encode(&Kept {
            record: rec(1, 5),
            acked_unix: None,
        });
        old.truncate(LEN_1);
        old[0] = VERSION_1;
        let again = encode(&decode([1; 32], &old).unwrap());
        assert_eq!(again[1..LEN_1], old[1..]);
        assert_eq!(again[0], VERSION);
    }

    fn with_fundings(n: u8, fundings: &[u8], more: bool) -> PaidRecord {
        PaidRecord {
            unconfirmed_fundings: fundings.iter().map(|t| [*t; 32]).collect(),
            more_fundings: more,
            ..rec(n, 5)
        }
    }

    /// SL-40: every funding of the channels not yet confirmed is kept with the
    /// event; the older versions still read, with none; the strict reading
    /// holds for the new bytes.
    #[test]
    fn sl_40_every_funding_is_kept_with_the_event_and_older_versions_read_without_it() {
        let kept_funded = Kept {
            record: with_fundings(1, &[7, 8, 9], false),
            acked_unix: None,
        };
        let bytes = encode(&kept_funded);
        assert_eq!(bytes[0], VERSION);
        // The count and the flag follow the fields of version 2.
        assert_eq!(bytes.len(), LEN_2 + 2 + 3 * 32);
        assert_eq!(&bytes[LEN_2..LEN_2 + 2], &[3, 0]);
        assert_eq!(&bytes[LEN_2 + 2..LEN_2 + 34], &[7; 32]);
        assert_eq!(&bytes[LEN_2 + 66..], &[9; 32]);
        assert_eq!(decode([1; 32], &bytes).unwrap(), kept_funded);
        // Version 2 is the front of version 3, and has no funding.
        let mut v2 = bytes[..LEN_2].to_vec();
        v2[0] = VERSION_2;
        let read = decode([1; 32], &v2).unwrap();
        assert!(read.record.unconfirmed_fundings.is_empty());
        assert!(!read.record.more_fundings);
        assert_eq!(read.record.skimmed_msat, kept_funded.record.skimmed_msat);
        // A length that is not the count's is refused.
        assert!(decode([1; 32], &bytes[..bytes.len() - 1]).is_err());
        assert!(decode([1; 32], &bytes[..LEN_2]).is_err());
        assert!(decode([1; 32], &v2[..LEN_2 - 1]).is_err());
        let mut long = v2.clone();
        long.push(0);
        assert!(decode([1; 32], &long).is_err());
        let mut long = bytes.clone();
        long.push(0);
        assert!(decode([1; 32], &long).is_err());
        // No funding is a count of 0 and nothing behind it.
        let none = encode(&Kept {
            record: rec(1, 5),
            acked_unix: None,
        });
        assert_eq!(none.len(), LEN_2 + 2);
        assert!(decode([1; 32], &none)
            .unwrap()
            .record
            .unconfirmed_fundings
            .is_empty());
        // The count is at most 8; the flag is 0 to 3, its first bit only with a full list.
        let eight = encode(&Kept {
            record: with_fundings(1, &[1, 2, 3, 4, 5, 6, 7, 8], true),
            acked_unix: None,
        });
        assert!(decode([1; 32], &eight).unwrap().record.more_fundings);
        // A lost funding with no name is the second bit, with any list; the
        // first bit still needs a full list.
        let lost = encode(&Kept {
            record: PaidRecord {
                lost_unknown: true,
                ..rec(1, 5)
            },
            acked_unix: None,
        });
        assert_eq!(lost[LEN_2 + 1], 2);
        let got = decode([1; 32], &lost).unwrap();
        assert!(got.record.lost_unknown && !got.record.more_fundings);
        assert_eq!(encode(&got), lost);
        let mut b = lost.clone();
        b[LEN_2 + 1] = 3;
        assert!(decode([1; 32], &b).is_err());
        let mut nine = eight.clone();
        nine[LEN_2] = 9;
        nine.extend_from_slice(&[9; 32]);
        assert!(decode([1; 32], &nine).is_err());
        for flag in [4u8, 255] {
            let mut b = bytes.clone();
            b[LEN_2 + 1] = flag;
            assert!(decode([1; 32], &b).is_err());
        }
        let mut b = bytes.clone();
        b[LEN_2 + 1] = 1;
        assert!(decode([1; 32], &b).is_err());
        // A funding twice is refused, so that the bytes are the ones `encode` makes.
        let twice = encode(&Kept {
            record: with_fundings(1, &[4, 4], false),
            acked_unix: None,
        });
        assert!(decode([1; 32], &twice).is_err());
    }

    #[cfg(feature = "testing")]
    #[test]
    fn sl_40_the_fuzz_hook_writes_back_the_version_it_read() {
        let kept = Kept {
            record: with_fundings(1, &[3, 4], false),
            acked_unix: Some(9),
        };
        let v3 = encode(&kept);
        assert_eq!(reencode_for_test([1; 32], &v3).unwrap(), v3);
        let mut v2 = v3[..LEN_2].to_vec();
        v2[0] = VERSION_2;
        assert_eq!(reencode_for_test([1; 32], &v2).unwrap(), v2);
        let mut v1 = v3[..LEN_1].to_vec();
        v1[0] = VERSION_1;
        assert_eq!(reencode_for_test([1; 32], &v1).unwrap(), v1);
    }

    #[tokio::test]
    async fn sl_40_a_funding_survives_a_restart_with_its_event() {
        let d = disks("paid-funding");
        let (log, flusher) = start(&d);
        let r = PaidRecord {
            unconfirmed_fundings: vec![[4; 32], [5; 32]],
            ..rec(6, 10)
        };
        log.record(r.clone()).await.unwrap();
        flusher.abort();
        drop(log);
        let (log, _f) = start(&d);
        assert_eq!(log.pending(NOW).await.unwrap(), vec![r]);
    }
}
