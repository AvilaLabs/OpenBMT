//! What the node expects of each invoice it made, kept in the two-copy store
//! (SL-20 to SL-25) so that a restart forgets none.
//!
//! For each invoice: the most the provider may skim from the payment for
//! opening a channel (SL-29) and when the invoice expires. A payment is
//! claimed only for an invoice that is here, not expired, with no more skimmed
//! than recorded.
//!
//! The whole book is one sealed blob. Each change is written through the
//! store the strict way, the second device first and then the counter's copy
//! (SL-24), and [`Expectations::record`] returns only when both have it, so an
//! invoice the counter shows is always known after a restart. Expired entries
//! are dropped when the book is loaded and on every write.

use crate::store::DualStore;
use lightning::io;
use lightning::util::persist::KVStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Where the book is kept.
pub const PRIMARY: &str = "invoice_expectations";
/// The key of the one blob.
pub const KEY: &str = "book";
const VERSION: u8 = 1;
const ENTRY_LEN: usize = 32 + 8 + 8;

/// What is expected of one invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expectation {
    /// The most the provider may skim from the payment, in millisatoshis.
    pub max_fee_msat: u64,
    /// When the invoice expires, in seconds since the epoch.
    pub expires_unix: u64,
}

/// What a payment's claim is judged to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// An invoice this node made, not expired, skim within the maximum.
    Allowed,
    /// Not an invoice this node made (or one it has dropped).
    Unknown,
    /// An invoice that has expired.
    Expired,
    /// More skimmed than the invoice allows (SL-29).
    SkimAboveMax,
}

/// The book of expectations.
pub struct Expectations {
    store: Arc<DualStore>,
    book: Mutex<HashMap<[u8; 32], Expectation>>,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn encode(book: &HashMap<[u8; 32], Expectation>) -> Vec<u8> {
    let mut entries: Vec<_> = book.iter().collect();
    entries.sort_by_key(|(h, _)| **h);
    let mut out = Vec::with_capacity(5 + entries.len() * ENTRY_LEN);
    out.push(VERSION);
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (hash, e) in entries {
        out.extend_from_slice(hash);
        out.extend_from_slice(&e.max_fee_msat.to_be_bytes());
        out.extend_from_slice(&e.expires_unix.to_be_bytes());
    }
    out
}

fn decode(bytes: &[u8]) -> Result<HashMap<[u8; 32], Expectation>, io::Error> {
    if bytes.len() < 5 || bytes[0] != VERSION {
        return Err(invalid("expectations: version"));
    }
    let count = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
    let body = &bytes[5..];
    if body.len()
        != count
            .checked_mul(ENTRY_LEN)
            .ok_or_else(|| invalid("count"))?
    {
        return Err(invalid("expectations: length"));
    }
    let mut book = HashMap::with_capacity(count);
    for i in 0..count {
        let chunk = &body[i * ENTRY_LEN..(i + 1) * ENTRY_LEN];
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&chunk[..32]);
        book.insert(
            hash,
            Expectation {
                max_fee_msat: u64::from_be_bytes(chunk[32..40].try_into().unwrap()),
                expires_unix: u64::from_be_bytes(chunk[40..48].try_into().unwrap()),
            },
        );
    }
    Ok(book)
}

fn drop_expired(book: &mut HashMap<[u8; 32], Expectation>, now: u64) -> bool {
    let before = book.len();
    book.retain(|_, e| e.expires_unix > now);
    book.len() != before
}

impl Expectations {
    /// Reads the book from the counter's copy (the newer one by the time the
    /// node reads it, SL-25) and drops what has expired, writing the book
    /// again if anything was dropped.
    pub async fn load(store: Arc<DualStore>, now: u64) -> Result<Self, io::Error> {
        let mut book = match KVStore::read(&*store, PRIMARY, "", KEY).await {
            Ok(bytes) => decode(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => return Err(e),
        };
        let dropped = drop_expired(&mut book, now);
        let blob = dropped.then(|| encode(&book));
        let me = Expectations {
            store,
            book: Mutex::new(book),
        };
        if let Some(blob) = blob {
            KVStore::write(&*me.store, PRIMARY, "", KEY, blob).await?;
        }
        Ok(me)
    }

    /// Records an invoice. Returns when both copies hold it. The numbering of
    /// the store's writes is taken while the book is locked, so the blobs
    /// land in the order the book changed.
    pub async fn record(
        &self,
        payment_hash: [u8; 32],
        expectation: Expectation,
        now: u64,
    ) -> Result<(), io::Error> {
        let write = {
            let mut book = self.book.lock().unwrap();
            drop_expired(&mut book, now);
            book.insert(payment_hash, expectation);
            KVStore::write(&*self.store, PRIMARY, "", KEY, encode(&book))
        };
        write.await
    }

    /// Takes an invoice out (it is paid), the same way.
    pub async fn forget(&self, payment_hash: &[u8; 32], now: u64) -> Result<(), io::Error> {
        let write = {
            let mut book = self.book.lock().unwrap();
            let removed = book.remove(payment_hash).is_some();
            let dropped = drop_expired(&mut book, now);
            if !removed && !dropped {
                return Ok(());
            }
            KVStore::write(&*self.store, PRIMARY, "", KEY, encode(&book))
        };
        write.await
    }

    /// Whether a payment of this hash, with this much skimmed, is claimed.
    pub fn judge(&self, payment_hash: &[u8; 32], skimmed_msat: u64, now: u64) -> Claim {
        match self.book.lock().unwrap().get(payment_hash) {
            None => Claim::Unknown,
            Some(e) if e.expires_unix <= now => Claim::Expired,
            Some(e) if skimmed_msat > e.max_fee_msat => Claim::SkimAboveMax,
            Some(_) => Claim::Allowed,
        }
    }

    /// How many invoices are in the book.
    pub fn len(&self) -> usize {
        self.book.lock().unwrap().len()
    }

    /// Whether the book is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bip85::NodeSeed;
    use crate::device::{DeviceStatus, DirDevice, GatedDevice, SecondDevice};
    use crate::logger::Logger;
    use crate::seal::{open, SealKey};
    use crate::testutil::temp_dir;
    use lightning::sign::KeysManager;
    use lightning::util::logger::Level;
    use lightning_persister::fs_store::FilesystemStore;
    use std::time::Duration;

    const NOW: u64 = 1_000_000;

    /// A counter's disk and a second device that outlive each "start".
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

    /// A fresh store over the same two disks: what a start makes.
    fn start(d: &Disks) -> Arc<DualStore> {
        Arc::new(DualStore::new(
            Arc::new(FilesystemStore::new(d.dir.join("local"))),
            Arc::new(d.device.clone()),
            d.key.clone(),
            Arc::new(KeysManager::new(&[1; 32], 1, 1, true)),
            Arc::new(DeviceStatus::new()),
            Arc::new(Logger::open(&d.dir.join("log"), Level::Debug).unwrap()),
        ))
    }

    fn exp(max_fee_msat: u64, expires_unix: u64) -> Expectation {
        Expectation {
            max_fee_msat,
            expires_unix,
        }
    }

    #[tokio::test]
    async fn sl_20_an_invoice_made_before_a_restart_is_claimed_after_it() {
        let d = disks("expect-restart");
        let book = Expectations::load(start(&d), NOW).await.unwrap();
        assert!(book.is_empty());
        book.record([7; 32], exp(1_000_000, NOW + 3600), NOW)
            .await
            .unwrap();
        assert_eq!(book.judge(&[7; 32], 1_000_000, NOW), Claim::Allowed);
        drop(book);
        // The node is stopped and started: a new store over the same disks.
        let again = Expectations::load(start(&d), NOW + 60).await.unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again.judge(&[7; 32], 1_000_000, NOW + 60), Claim::Allowed);
        assert_eq!(again.judge(&[7; 32], 0, NOW + 60), Claim::Allowed);
        // An invoice this node never made is not claimed.
        assert_eq!(again.judge(&[8; 32], 0, NOW + 60), Claim::Unknown);
    }

    #[tokio::test]
    async fn sl_29_a_skim_above_the_recorded_maximum_is_refused_after_a_restart() {
        let d = disks("expect-skim");
        let book = Expectations::load(start(&d), NOW).await.unwrap();
        book.record([1; 32], exp(1_000_000, NOW + 3600), NOW)
            .await
            .unwrap();
        book.record([2; 32], exp(0, NOW + 3600), NOW).await.unwrap();
        drop(book);
        let again = Expectations::load(start(&d), NOW + 1).await.unwrap();
        assert_eq!(
            again.judge(&[1; 32], 1_000_001, NOW + 1),
            Claim::SkimAboveMax
        );
        assert_eq!(again.judge(&[1; 32], 1_000_000, NOW + 1), Claim::Allowed);
        assert_eq!(again.judge(&[2; 32], 1, NOW + 1), Claim::SkimAboveMax);
    }

    #[tokio::test]
    async fn sl_20_an_expired_entry_is_dropped_on_start_and_on_a_write() {
        let d = disks("expect-expiry");
        let book = Expectations::load(start(&d), NOW).await.unwrap();
        book.record([1; 32], exp(0, NOW + 10), NOW).await.unwrap();
        book.record([2; 32], exp(0, NOW + 100), NOW).await.unwrap();
        // A write after the first has expired drops it.
        book.record([3; 32], exp(0, NOW + 200), NOW + 20)
            .await
            .unwrap();
        assert_eq!(book.len(), 2);
        assert_eq!(book.judge(&[1; 32], 0, NOW + 20), Claim::Unknown);
        // Not claimed on the instant of expiry, even before it is dropped.
        assert_eq!(book.judge(&[2; 32], 0, NOW + 100), Claim::Expired);
        drop(book);
        // On start, what has expired by then is dropped, and the blob is
        // written again on both copies without it.
        let again = Expectations::load(start(&d), NOW + 150).await.unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again.judge(&[2; 32], 0, NOW + 150), Claim::Unknown);
        assert_eq!(again.judge(&[3; 32], 0, NOW + 150), Claim::Allowed);
        let name = crate::store::device_name(PRIMARY, "", KEY);
        let sealed = d.device.read(&name).await.unwrap().unwrap();
        let on_device = decode(&open(&d.key, &name, &sealed).unwrap()).unwrap();
        assert_eq!(on_device.len(), 1);
        assert!(on_device.contains_key(&[3; 32]));
    }

    #[tokio::test]
    async fn sl_24_the_book_goes_to_the_second_device_first_and_the_invoice_waits_for_it() {
        let d = disks("expect-order");
        let book = Arc::new(Expectations::load(start(&d), NOW).await.unwrap());
        d.device.set_away(true);
        let b = Arc::clone(&book);
        let write = tokio::spawn(async move { b.record([4; 32], exp(0, NOW + 3600), NOW).await });
        tokio::time::sleep(Duration::from_millis(700)).await;
        // Not handed back, and the counter's own copy is not ahead of the device's.
        assert!(!write.is_finished());
        assert!(!d.dir.join("local").join(PRIMARY).join(KEY).exists());
        d.device.set_away(false);
        tokio::time::timeout(Duration::from_secs(10), write)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(d.dir.join("local").join(PRIMARY).join(KEY).exists());
        let name = crate::store::device_name(PRIMARY, "", KEY);
        assert!(d.device.read(&name).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn sl_20_forgetting_a_paid_invoice_is_kept_across_a_restart() {
        let d = disks("expect-forget");
        let book = Expectations::load(start(&d), NOW).await.unwrap();
        book.record([1; 32], exp(0, NOW + 3600), NOW).await.unwrap();
        book.forget(&[1; 32], NOW).await.unwrap();
        // Nothing to forget: no write, no error.
        book.forget(&[9; 32], NOW).await.unwrap();
        drop(book);
        let again = Expectations::load(start(&d), NOW).await.unwrap();
        assert!(again.is_empty());
    }

    #[test]
    fn sl_20_a_damaged_blob_is_an_error_not_an_empty_book() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[2, 0, 0, 0, 0]).is_err());
        assert!(decode(&[1, 0, 0, 0, 1]).is_err());
        let mut book = HashMap::new();
        book.insert([1; 32], exp(5, 9));
        assert_eq!(decode(&encode(&book)).unwrap(), book);
    }
}
