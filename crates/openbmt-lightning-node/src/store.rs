//! The node's store: every change written to the second device and to the
//! counter's own disk (SL-20), the second device first (SL-24).
//!
//! It is the async `KVStore` the library's async persister uses
//! (`MonitorUpdatingPersisterAsync` on `ChainMonitor::new_async_beta`, SL-21).
//! The persister answers `InProgress` and the library calls
//! `channel_monitor_updated` for a write only when its future is done, so a
//! write is done only when BOTH copies are written: the future does the
//! second device's write, and only then the local one.
//!
//! A device that is away does not fail the write and nothing panics (SL-23):
//! the future waits and tries again, with the status the counter reads set to
//! away. Payments in flight finish when the device returns.
//!
//! Writes to one name are applied in the order they were asked for, as the
//! trait requires: each is given a number when it is asked for, and one that a
//! later write has already replaced is skipped.

use crate::device::{DeviceStatus, SecondDevice};
use crate::logger::Logger;
use crate::seal::{open, seal, SealKey, NONCE_LEN};
use lightning::io;
use lightning::sign::{EntropySource, KeysManager};
use lightning::util::async_poll::AsyncResult;
use lightning::util::logger::Level;
use lightning::util::persist::KVStore;
use lightning_persister::fs_store::FilesystemStore;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The name of a blob on the second device: the key's three parts joined by `~`.
pub fn device_name(primary: &str, secondary: &str, key: &str) -> String {
    format!("{primary}~{secondary}~{key}")
}

/// The three parts of a key, from a device name.
pub fn split_device_name(name: &str) -> Option<(&str, &str, &str)> {
    let mut parts = name.splitn(3, '~');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn other(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.to_string())
}

fn nonce(entropy: &KeysManager) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n.copy_from_slice(&entropy.get_secure_random_bytes()[..NONCE_LEN]);
    n
}

struct Inner {
    local: Arc<FilesystemStore>,
    device: Arc<dyn SecondDevice>,
    key: SealKey,
    entropy: Arc<KeysManager>,
    status: Arc<DeviceStatus>,
    logger: Arc<Logger>,
    tickets: AtomicU64,
    // The number of the last write applied to each name. Held across the
    // second device's wait, so that writes go one at a time, in order.
    applied: tokio::sync::Mutex<HashMap<String, u64>>,
}

/// Decrements the pending count when the write's future is done or dropped.
struct Pending(Arc<DeviceStatus>);

impl Pending {
    fn new(status: &Arc<DeviceStatus>) -> Self {
        status.begin_write();
        Pending(Arc::clone(status))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.end_write();
    }
}

enum Op {
    Write(Vec<u8>),
    Remove { lazy: bool },
}

impl Inner {
    /// Retries until the second device has the blob.
    async fn to_device(&self, name: &str, op: &Op) {
        let sealed = match op {
            Op::Write(buf) => Some(seal(&self.key, nonce(&self.entropy), name, buf)),
            Op::Remove { .. } => None,
        };
        let mut delay = Duration::from_millis(200);
        let mut first = true;
        loop {
            let result = match &sealed {
                Some(blob) => self.device.write(name, blob.clone()).await,
                None => self.device.remove(name).await,
            };
            match result {
                Ok(()) => {
                    self.status.stored(now_unix());
                    if !first {
                        self.logger.line(Level::Info, "second device is back");
                    }
                    return;
                }
                Err(e) => {
                    self.status.failed();
                    if first {
                        self.logger.line(
                            Level::Warn,
                            &format!("second device unreachable ({e}); the write waits"),
                        );
                        first = false;
                    }
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(3));
                }
            }
        }
    }

    async fn apply(
        &self,
        ticket: u64,
        p: String,
        s: String,
        k: String,
        op: Op,
    ) -> Result<(), io::Error> {
        let name = device_name(&p, &s, &k);
        let mut applied = self.applied.lock().await;
        if applied.get(&name).is_some_and(|done| *done > ticket) {
            // A later write to this name is already in both places.
            return Ok(());
        }
        self.to_device(&name, &op).await;
        match op {
            Op::Write(buf) => KVStore::write(&*self.local, &p, &s, &k, buf).await?,
            Op::Remove { lazy } => KVStore::remove(&*self.local, &p, &s, &k, lazy).await?,
        }
        applied.insert(name, ticket);
        Ok(())
    }
}

/// The store the node runs on.
#[derive(Clone)]
pub struct DualStore {
    inner: Arc<Inner>,
}

impl DualStore {
    /// A store of the counter's own disk at `local` and the second device.
    pub fn new(
        local: Arc<FilesystemStore>,
        device: Arc<dyn SecondDevice>,
        key: SealKey,
        entropy: Arc<KeysManager>,
        status: Arc<DeviceStatus>,
        logger: Arc<Logger>,
    ) -> Self {
        DualStore {
            inner: Arc::new(Inner {
                local,
                device,
                key,
                entropy,
                status,
                logger,
                tickets: AtomicU64::new(0),
                applied: tokio::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    fn queue(&self, p: &str, s: &str, k: &str, op: Op) -> AsyncResult<'static, (), io::Error> {
        let inner = Arc::clone(&self.inner);
        let ticket = inner.tickets.fetch_add(1, Ordering::SeqCst);
        let pending = Pending::new(&inner.status);
        let (p, s, k) = (p.to_string(), s.to_string(), k.to_string());
        Box::pin(async move {
            let _pending = pending;
            inner.apply(ticket, p, s, k, op).await
        })
    }

    /// The second device's status.
    pub fn status(&self) -> Arc<DeviceStatus> {
        Arc::clone(&self.inner.status)
    }
}

impl KVStore for DualStore {
    fn read(&self, p: &str, s: &str, k: &str) -> AsyncResult<'static, Vec<u8>, io::Error> {
        // Reads are of the counter's own copy; the second device is read only
        // by `state::compare` and `state::pull`.
        KVStore::read(&*self.inner.local, p, s, k)
    }

    fn write(
        &self,
        p: &str,
        s: &str,
        k: &str,
        buf: Vec<u8>,
    ) -> AsyncResult<'static, (), io::Error> {
        self.queue(p, s, k, Op::Write(buf))
    }

    fn remove(&self, p: &str, s: &str, k: &str, lazy: bool) -> AsyncResult<'static, (), io::Error> {
        self.queue(p, s, k, Op::Remove { lazy })
    }

    fn list(&self, p: &str, s: &str) -> AsyncResult<'static, Vec<String>, io::Error> {
        KVStore::list(&*self.inner.local, p, s)
    }
}

/// The second device read as a `KVStore`: what `state` compares and copies.
/// One try for each call; an error is an error.
#[derive(Clone)]
pub struct DeviceView {
    device: Arc<dyn SecondDevice>,
    key: SealKey,
    entropy: Arc<KeysManager>,
}

impl DeviceView {
    /// A view of `device` with the node's key.
    pub fn new(device: Arc<dyn SecondDevice>, key: SealKey, entropy: Arc<KeysManager>) -> Self {
        Self {
            device,
            key,
            entropy,
        }
    }

    /// Every name on the device.
    pub async fn names(&self) -> Result<Vec<String>, io::Error> {
        self.device.list().await.map_err(|e| other(&e.0))
    }
}

impl KVStore for DeviceView {
    fn read(&self, p: &str, s: &str, k: &str) -> AsyncResult<'static, Vec<u8>, io::Error> {
        let (device, key, name) = (
            Arc::clone(&self.device),
            self.key.clone(),
            device_name(p, s, k),
        );
        Box::pin(async move {
            match device.read(&name).await {
                Ok(Some(sealed)) => open(&key, &name, &sealed).map_err(|_| other("seal")),
                Ok(None) => Err(io::Error::new(io::ErrorKind::NotFound, "none")),
                Err(e) => Err(other(&e.0)),
            }
        })
    }

    fn write(
        &self,
        p: &str,
        s: &str,
        k: &str,
        buf: Vec<u8>,
    ) -> AsyncResult<'static, (), io::Error> {
        let (device, name) = (Arc::clone(&self.device), device_name(p, s, k));
        let sealed = seal(&self.key, nonce(&self.entropy), &name, &buf);
        Box::pin(async move { device.write(&name, sealed).await.map_err(|e| other(&e.0)) })
    }

    fn remove(
        &self,
        p: &str,
        s: &str,
        k: &str,
        _lazy: bool,
    ) -> AsyncResult<'static, (), io::Error> {
        let (device, name) = (Arc::clone(&self.device), device_name(p, s, k));
        Box::pin(async move { device.remove(&name).await.map_err(|e| other(&e.0)) })
    }

    fn list(&self, p: &str, s: &str) -> AsyncResult<'static, Vec<String>, io::Error> {
        let (device, prefix) = (Arc::clone(&self.device), format!("{p}~{s}~"));
        Box::pin(async move {
            let names = device.list().await.map_err(|e| other(&e.0))?;
            Ok(names
                .into_iter()
                .filter_map(|n| n.strip_prefix(&prefix).map(str::to_string))
                .collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bip85::NodeSeed;
    use crate::device::{DeviceState, DirDevice, GatedDevice};
    use crate::testutil::temp_dir;
    use lightning::util::logger::Level;

    struct Rig {
        store: DualStore,
        device: GatedDevice<DirDevice>,
        local: Arc<FilesystemStore>,
        local_dir: std::path::PathBuf,
        status: Arc<DeviceStatus>,
        key: SealKey,
    }

    fn rig(name: &str) -> Rig {
        let dir = temp_dir(name);
        let local_dir = dir.join("local");
        let device = GatedDevice::new(DirDevice::new(dir.join("device")).unwrap());
        let local = Arc::new(FilesystemStore::new(local_dir.clone()));
        let seed = NodeSeed([5; 64]);
        let key = SealKey::from_seed(&seed);
        let keys = Arc::new(KeysManager::new(&[1; 32], 1, 1, true));
        let status = Arc::new(DeviceStatus::new());
        let logger = Arc::new(Logger::open(&dir.join("log"), Level::Debug).unwrap());
        let store = DualStore::new(
            Arc::clone(&local),
            Arc::new(device.clone()),
            key.clone(),
            keys,
            Arc::clone(&status),
            logger,
        );
        Rig {
            store,
            device,
            local,
            local_dir,
            status,
            key,
        }
    }

    async fn settle(ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    #[tokio::test]
    async fn sl_21_a_write_is_done_only_when_both_copies_are_written() {
        let r = rig("store-both");
        r.store
            .write("monitors", "", "a_0", b"v1".to_vec())
            .await
            .unwrap();
        // The device has the sealed blob, and it is not the plain one.
        let sealed = r.device.read("monitors~~a_0").await.unwrap().unwrap();
        assert_ne!(sealed, b"v1");
        assert_eq!(open(&r.key, "monitors~~a_0", &sealed).unwrap(), b"v1");
        assert_eq!(
            KVStore::read(&*r.local, "monitors", "", "a_0")
                .await
                .unwrap(),
            b"v1"
        );
        assert_eq!(r.status.snapshot().stored_writes, 1);
        assert_eq!(r.status.snapshot().pending_writes, 0);
    }

    #[tokio::test]
    async fn sl_24_the_second_device_is_written_first_and_a_device_away_holds_the_write() {
        let r = rig("store-order");
        r.device.set_away(true);
        let store = r.store.clone();
        let write = tokio::spawn(async move {
            store
                .write("monitor_updates", "a_0", "0000000001", b"u".to_vec())
                .await
        });
        settle(700).await;
        // Still in progress, nothing panicked, and the counter's own copy is
        // not written ahead of the device's.
        assert!(!write.is_finished());
        assert!(!r.local_dir.join("monitor_updates/a_0/0000000001").exists());
        let snap = r.status.snapshot();
        assert_eq!(snap.state, DeviceState::Away);
        assert_eq!(snap.pending_writes, 1);
        // The device returns: the write finishes, device first then local.
        r.device.set_away(false);
        tokio::time::timeout(Duration::from_secs(10), write)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(r.local_dir.join("monitor_updates/a_0/0000000001").exists());
        assert!(r
            .device
            .inner()
            .dir()
            .join("monitor_updates~a_0~0000000001")
            .exists());
        let snap = r.status.snapshot();
        assert_eq!(snap.state, DeviceState::Reachable);
        assert_eq!(snap.pending_writes, 0);
        assert!(snap.last_stored_unix > 0);
    }

    #[tokio::test]
    async fn sl_23_a_long_absence_is_not_an_error() {
        let r = rig("store-long");
        r.device.set_away(true);
        let store = r.store.clone();
        let write =
            tokio::spawn(async move { store.write("", "", "manager", b"m".to_vec()).await });
        settle(1500).await;
        assert!(!write.is_finished());
        r.device.set_away(false);
        tokio::time::timeout(Duration::from_secs(10), write)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn sl_21_a_later_write_to_a_name_is_never_overwritten_by_an_earlier_one() {
        let r = rig("store-order-same");
        let first = r.store.write("", "", "manager", b"first".to_vec());
        let second = r.store.write("", "", "manager", b"second".to_vec());
        second.await.unwrap();
        first.await.unwrap();
        assert_eq!(
            KVStore::read(&*r.local, "", "", "manager").await.unwrap(),
            b"second"
        );
        let sealed = r.device.read("~~manager").await.unwrap().unwrap();
        assert_eq!(open(&r.key, "~~manager", &sealed).unwrap(), b"second");
    }

    #[tokio::test]
    async fn sl_20_removes_go_to_both_copies() {
        let r = rig("store-remove");
        r.store
            .write("monitor_updates", "a_0", "1", b"u".to_vec())
            .await
            .unwrap();
        r.store
            .remove("monitor_updates", "a_0", "1", false)
            .await
            .unwrap();
        assert!(r
            .device
            .read("monitor_updates~a_0~1")
            .await
            .unwrap()
            .is_none());
        assert!(KVStore::read(&*r.local, "monitor_updates", "a_0", "1")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn sl_20_the_device_view_opens_what_the_store_wrote_and_pull_and_push_agree() {
        let r = rig("store-view");
        r.store
            .write("monitors", "", "a_0", b"mon".to_vec())
            .await
            .unwrap();
        r.store
            .write("monitor_updates", "a_0", "2", b"upd".to_vec())
            .await
            .unwrap();
        r.store
            .write("", "", "manager", b"mgr".to_vec())
            .await
            .unwrap();
        let keys = Arc::new(KeysManager::new(&[1; 32], 1, 1, true));
        let view = DeviceView::new(Arc::new(r.device.clone()), r.key.clone(), keys);
        assert_eq!(view.read("monitors", "", "a_0").await.unwrap(), b"mon");
        assert_eq!(
            view.list("monitor_updates", "a_0").await.unwrap(),
            vec!["2".to_string()]
        );
        // A wiped counter is brought back from the device alone.
        let wiped = temp_dir("store-view-wiped");
        let fresh = FilesystemStore::new(wiped.clone());
        let n = crate::state::pull(&wiped, &fresh, &view).await.unwrap();
        assert_eq!(n, 3);
        assert_eq!(
            KVStore::read(&fresh, "monitor_updates", "a_0", "2")
                .await
                .unwrap(),
            b"upd"
        );
        assert_eq!(
            KVStore::read(&fresh, "", "", "manager").await.unwrap(),
            b"mgr"
        );
        // And a device that is behind is brought up to the counter's copy.
        fresh
            .write("monitors", "", "b_1", b"newer".to_vec())
            .await
            .unwrap();
        crate::state::push(&wiped, &fresh, &view).await.unwrap();
        assert_eq!(view.read("monitors", "", "b_1").await.unwrap(), b"newer");
    }
}
