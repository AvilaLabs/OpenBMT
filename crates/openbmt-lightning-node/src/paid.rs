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
const VERSION: u8 = 1;
const LEN: usize = 1 + 8 + 8 + 1 + 8 + 1 + 8;

/// A paid sale as it is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidRecord {
    /// The payment hash of the invoice.
    pub payment_hash: [u8; 32],
    /// What arrived, in millisatoshis.
    pub amount_msat: u64,
    /// What the payer sent, if the library knew it.
    pub sender_intended_msat: Option<u64>,
    /// When the claim was seen, in seconds since the epoch. A repeat carries
    /// the time of the first.
    pub received_unix: u64,
}

fn hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(name: &str) -> Option<[u8; 32]> {
    if name.len() != 64 || !name.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&name[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// What is kept for a hash: the event, and when it was acknowledged if it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kept {
    record: PaidRecord,
    acked_unix: Option<u64>,
}

fn encode(k: &Kept) -> Vec<u8> {
    let r = &k.record;
    let mut out = Vec::with_capacity(LEN);
    out.push(VERSION);
    out.extend_from_slice(&r.amount_msat.to_be_bytes());
    out.extend_from_slice(&r.received_unix.to_be_bytes());
    out.push(r.sender_intended_msat.is_some() as u8);
    out.extend_from_slice(&r.sender_intended_msat.unwrap_or(0).to_be_bytes());
    out.push(k.acked_unix.is_some() as u8);
    out.extend_from_slice(&k.acked_unix.unwrap_or(0).to_be_bytes());
    out
}

fn decode(hash: [u8; 32], bytes: &[u8]) -> Result<Kept, io::Error> {
    if bytes.len() != LEN || bytes[0] != VERSION {
        return Err(invalid("paid event"));
    }
    let sender = u64::from_be_bytes(bytes[18..26].try_into().unwrap());
    let acked = u64::from_be_bytes(bytes[27..35].try_into().unwrap());
    Ok(Kept {
        record: PaidRecord {
            payment_hash: hash,
            amount_msat: u64::from_be_bytes(bytes[1..9].try_into().unwrap()),
            received_unix: u64::from_be_bytes(bytes[9..17].try_into().unwrap()),
            sender_intended_msat: (bytes[17] == 1).then_some(sender),
        },
        acked_unix: (bytes[26] == 1).then_some(acked),
    })
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
        let kept = Kept {
            record: r,
            acked_unix: None,
        };
        KVStore::write(
            &*self.store,
            PRIMARY,
            "",
            &hex(&r.payment_hash),
            encode(&kept),
        )
        .await?;
        Ok(Some(r))
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
    use crate::bip85::NodeSeed;
    use crate::device::{DeviceStatus, DirDevice, GatedDevice, SecondDevice};
    use crate::logger::Logger;
    use crate::seal::{open, SealKey};
    use crate::store::{device_name, DualStore};
    use crate::testutil::temp_dir;
    use lightning::sign::KeysManager;
    use lightning::util::logger::Level;
    use lightning_persister::fs_store::FilesystemStore;
    use std::time::Duration;

    struct Disks {
        dir: std::path::PathBuf,
        device: GatedDevice<DirDevice>,
        key: SealKey,
    }

    fn disks(name: &str) -> Disks {
        let dir = temp_dir(name);
        Disks {
            device: GatedDevice::new(DirDevice::new(dir.join("device")).unwrap()),
            key: SealKey::from_seed(&NodeSeed([5; 64])),
            dir,
        }
    }

    /// A fresh log over the same disks, with the flusher running: a start.
    fn start(d: &Disks) -> (PaidLog, tokio::task::JoinHandle<()>) {
        let store = Arc::new(DualStore::new(
            Arc::new(FilesystemStore::new(d.dir.join("local"))),
            Arc::new(d.device.clone()),
            d.key.clone(),
            Arc::new(KeysManager::new(&[1; 32], 1, 1, true)),
            Arc::new(DeviceStatus::new()),
            Arc::new(Logger::open(&d.dir.join("log"), Level::Debug).unwrap()),
        ));
        let flusher = store.spawn_lazy_flusher(&tokio::runtime::Handle::current());
        (PaidLog::new(Arc::new(Background(store))), flusher)
    }

    fn rec(n: u8, at: u64) -> PaidRecord {
        PaidRecord {
            payment_hash: [n; 32],
            amount_msat: 49_000_000,
            sender_intended_msat: Some(50_000_000),
            received_unix: at,
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
    }
}
