//! The second device (SL-20): another shop device that keeps an encrypted
//! copy of every change of the node's state.
//!
//! The device holds opaque blobs under names and nothing else; it never has
//! the key (the blobs are sealed, `seal.rs`) and does not know what they are.
//! Two implementations here: [`DirDevice`], a directory, for tests and for a
//! device on the same machine, and the TCP one in `device_tcp.rs`.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Why the device did not do what was asked: it is away, or it refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceError(pub String);

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeviceError {}

/// The future every call of a device returns.
pub type DeviceFuture<T> = Pin<Box<dyn Future<Output = Result<T, DeviceError>> + Send + 'static>>;

/// A second device. A `write` that returns `Ok` has put the blob where it
/// survives the device being switched off.
pub trait SecondDevice: Send + Sync + 'static {
    /// Stores `blob` under `name`, replacing what was there.
    fn write(&self, name: &str, blob: Vec<u8>) -> DeviceFuture<()>;
    /// The blob under `name`, or `None` if there is none.
    fn read(&self, name: &str) -> DeviceFuture<Option<Vec<u8>>>;
    /// Every name stored.
    fn list(&self) -> DeviceFuture<Vec<String>>;
    /// Removes the blob under `name`; `Ok` also when there was none.
    fn remove(&self, name: &str) -> DeviceFuture<()>;
}

/// The characters a blob's name may have: the ones the library's keys use,
/// and `~`, which joins the parts of a key. Nothing that is a path.
pub fn name_is_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'~')
}

/// A device that is a directory.
#[derive(Debug, Clone)]
pub struct DirDevice {
    dir: PathBuf,
}

impl DirDevice {
    /// A device in `dir`, which is made if it is not there.
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// The directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, name: &str) -> Result<PathBuf, DeviceError> {
        if name_is_valid(name) {
            Ok(self.dir.join(name))
        } else {
            Err(DeviceError("name".into()))
        }
    }
}

fn io_err(e: std::io::Error) -> DeviceError {
    DeviceError(e.to_string())
}

fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, DeviceError> + Send + 'static,
) -> DeviceFuture<T> {
    Box::pin(async move {
        tokio::task::spawn_blocking(f).await.map_err(|e| DeviceError(e.to_string()))?
    })
}

fn write_atomic(dir: &Path, name: &str, blob: &[u8]) -> Result<(), DeviceError> {
    use std::io::Write;
    let tmp = dir.join(format!(".tmp-{name}"));
    let mut file = std::fs::File::create(&tmp).map_err(io_err)?;
    file.write_all(blob).map_err(io_err)?;
    file.sync_all().map_err(io_err)?;
    std::fs::rename(&tmp, dir.join(name)).map_err(io_err)?;
    // The rename is only kept once the directory is flushed.
    std::fs::File::open(dir).and_then(|d| d.sync_all()).map_err(io_err)
}

impl SecondDevice for DirDevice {
    fn write(&self, name: &str, blob: Vec<u8>) -> DeviceFuture<()> {
        let dir = self.dir.clone();
        match self.path(name) {
            Ok(_) => {
                let name = name.to_string();
                blocking(move || write_atomic(&dir, &name, &blob))
            }
            Err(e) => Box::pin(async move { Err(e) }),
        }
    }

    fn read(&self, name: &str) -> DeviceFuture<Option<Vec<u8>>> {
        match self.path(name) {
            Ok(path) => blocking(move || match std::fs::read(path) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(io_err(e)),
            }),
            Err(e) => Box::pin(async move { Err(e) }),
        }
    }

    fn list(&self) -> DeviceFuture<Vec<String>> {
        let dir = self.dir.clone();
        blocking(move || {
            let mut names = Vec::new();
            for entry in std::fs::read_dir(dir).map_err(io_err)? {
                let name = entry.map_err(io_err)?.file_name().to_string_lossy().into_owned();
                if name_is_valid(&name) {
                    names.push(name);
                }
            }
            names.sort();
            Ok(names)
        })
    }

    fn remove(&self, name: &str) -> DeviceFuture<()> {
        match self.path(name) {
            Ok(path) => blocking(move || match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(io_err(e)),
            }),
            Err(e) => Box::pin(async move { Err(e) }),
        }
    }
}

/// A device that can be made to be away, for tests: while it is away every
/// call fails as an unreachable device does.
#[derive(Clone)]
pub struct GatedDevice<D: SecondDevice> {
    inner: Arc<D>,
    away: Arc<AtomicBool>,
}

impl<D: SecondDevice> GatedDevice<D> {
    /// Wraps `inner`; it starts reachable.
    pub fn new(inner: D) -> Self {
        Self { inner: Arc::new(inner), away: Arc::new(AtomicBool::new(false)) }
    }

    /// Makes the device away or back.
    pub fn set_away(&self, away: bool) {
        self.away.store(away, Ordering::SeqCst);
    }

    /// The device behind the gate.
    pub fn inner(&self) -> &D {
        &self.inner
    }

    fn gate<T: Send + 'static>(&self, f: impl FnOnce() -> DeviceFuture<T>) -> DeviceFuture<T> {
        if self.away.load(Ordering::SeqCst) {
            Box::pin(async { Err(DeviceError("away".into())) })
        } else {
            f()
        }
    }
}

impl<D: SecondDevice> SecondDevice for GatedDevice<D> {
    fn write(&self, name: &str, blob: Vec<u8>) -> DeviceFuture<()> {
        self.gate(|| self.inner.write(name, blob))
    }
    fn read(&self, name: &str) -> DeviceFuture<Option<Vec<u8>>> {
        self.gate(|| self.inner.read(name))
    }
    fn list(&self) -> DeviceFuture<Vec<String>> {
        self.gate(|| self.inner.list())
    }
    fn remove(&self, name: &str) -> DeviceFuture<()> {
        self.gate(|| self.inner.remove(name))
    }
}

/// Whether the second device is reachable, as the counter can read it (SL-23, SL-45).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// The last call reached it.
    Reachable,
    /// The last call did not; writes wait.
    Away,
}

/// What the manager reads about the second device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceSnapshot {
    /// Reachable or away.
    pub state: DeviceState,
    /// Writes that have not yet reached it.
    pub pending_writes: usize,
    /// When it last stored a write, in seconds since the epoch; 0 if never.
    pub last_stored_unix: u64,
    /// Writes it has stored since the node started.
    pub stored_writes: u64,
}

/// The live status of the second device, shared between the writer and the
/// code that reads it.
#[derive(Debug, Default)]
pub struct DeviceStatus {
    away: AtomicBool,
    pending: AtomicUsize,
    last_stored: AtomicU64,
    stored: AtomicU64,
}

impl DeviceStatus {
    /// A status that reads reachable, with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin_write(&self) {
        self.pending.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn end_write(&self) {
        self.pending.fetch_sub(1, Ordering::SeqCst);
    }

    pub(crate) fn stored(&self, now_unix: u64) {
        self.away.store(false, Ordering::SeqCst);
        self.last_stored.store(now_unix, Ordering::SeqCst);
        self.stored.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn failed(&self) {
        self.away.store(true, Ordering::SeqCst);
    }

    /// The status now.
    pub fn snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            state: if self.away.load(Ordering::SeqCst) {
                DeviceState::Away
            } else {
                DeviceState::Reachable
            },
            pending_writes: self.pending.load(Ordering::SeqCst),
            last_stored_unix: self.last_stored.load(Ordering::SeqCst),
            stored_writes: self.stored.load(Ordering::SeqCst),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testutil::temp_dir as temp;

    #[tokio::test]
    async fn sl_20_a_directory_device_keeps_lists_and_removes() {
        let dir = temp("dir-device");
        let dev = DirDevice::new(&dir).unwrap();
        assert_eq!(dev.read("a~~b").await.unwrap(), None);
        dev.write("a~~b", b"one".to_vec()).await.unwrap();
        dev.write("a~~b", b"two".to_vec()).await.unwrap();
        dev.write("c", b"x".to_vec()).await.unwrap();
        assert_eq!(dev.read("a~~b").await.unwrap(), Some(b"two".to_vec()));
        assert_eq!(dev.list().await.unwrap(), vec!["a~~b".to_string(), "c".to_string()]);
        dev.remove("c").await.unwrap();
        dev.remove("c").await.unwrap();
        assert_eq!(dev.list().await.unwrap(), vec!["a~~b".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn sl_20_a_name_that_is_a_path_is_refused() {
        let dir = temp("dir-names");
        let dev = DirDevice::new(&dir).unwrap();
        for bad in ["", "../x", "a/b", ".hidden", "a b"] {
            assert!(dev.write(bad, vec![1]).await.is_err(), "{bad}");
            assert!(dev.read(bad).await.is_err(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn sl_23_a_gated_device_that_is_away_fails_every_call() {
        let dir = temp("gated");
        let dev = GatedDevice::new(DirDevice::new(&dir).unwrap());
        dev.write("k", vec![1]).await.unwrap();
        dev.set_away(true);
        assert!(dev.write("k", vec![2]).await.is_err());
        assert!(dev.read("k").await.is_err());
        assert!(dev.list().await.is_err());
        dev.set_away(false);
        assert_eq!(dev.read("k").await.unwrap(), Some(vec![1]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
