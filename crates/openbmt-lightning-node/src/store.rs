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
        &self, ticket: u64, p: String, s: String, k: String, op: Op,
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
        local: Arc<FilesystemStore>, device: Arc<dyn SecondDevice>, key: SealKey,
        entropy: Arc<KeysManager>, status: Arc<DeviceStatus>, logger: Arc<Logger>,
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
        &self, p: &str, s: &str, k: &str, buf: Vec<u8>,
    ) -> AsyncResult<'static, (), io::Error> {
        self.queue(p, s, k, Op::Write(buf))
    }

    fn remove(
        &self, p: &str, s: &str, k: &str, lazy: bool,
    ) -> AsyncResult<'static, (), io::Error> {
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
        Self { device, key, entropy }
    }

    /// Every name on the device.
    pub async fn names(&self) -> Result<Vec<String>, io::Error> {
        self.device.list().await.map_err(|e| other(&e.0))
    }
}

impl KVStore for DeviceView {
    fn read(&self, p: &str, s: &str, k: &str) -> AsyncResult<'static, Vec<u8>, io::Error> {
        let (device, key, name) = (Arc::clone(&self.device), self.key.clone(), device_name(p, s, k));
        Box::pin(async move {
            match device.read(&name).await {
                Ok(Some(sealed)) => open(&key, &name, &sealed).map_err(|_| other("seal")),
                Ok(None) => Err(io::Error::new(io::ErrorKind::NotFound, "none")),
                Err(e) => Err(other(&e.0)),
            }
        })
    }

    fn write(
        &self, p: &str, s: &str, k: &str, buf: Vec<u8>,
    ) -> AsyncResult<'static, (), io::Error> {
        let (device, name) = (Arc::clone(&self.device), device_name(p, s, k));
        let sealed = seal(&self.key, nonce(&self.entropy), &name, &buf);
        Box::pin(async move { device.write(&name, sealed).await.map_err(|e| other(&e.0)) })
    }

    fn remove(
        &self, p: &str, s: &str, k: &str, _lazy: bool,
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
