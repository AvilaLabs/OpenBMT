//! The files the counter keeps, as the program sees them. With the `network`
//! feature these are the standard library's own, so nothing differs there.
//! Without it (the build for a browser, which has no disk) the same calls
//! work on files held in memory, under the paths they are given, and the
//! time a file was written is the one [`set_now`] last gave.

#[cfg(feature = "network")]
pub use openbmt_net_io::{DirLock, FileStorage};
#[cfg(feature = "network")]
pub use std::fs::{self, File, OpenOptions};

/// The files carry the system's own times, so there is no time to give.
#[cfg(feature = "network")]
pub fn set_now(_secs: u64) {}

#[cfg(not(feature = "network"))]
pub use mem::{fs, set_now, DirLock, File, FileStorage, OpenOptions};

/// The files in memory, with the shape of the standard library's.
#[cfg(not(feature = "network"))]
mod mem {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct Data {
        bytes: Vec<u8>,
        modified: u64,
    }

    #[derive(Clone)]
    enum Node {
        Dir,
        File(Arc<Mutex<Data>>),
    }

    static NODES: Mutex<BTreeMap<PathBuf, Node>> = Mutex::new(BTreeMap::new());
    static NOW: AtomicU64 = AtomicU64::new(0);

    /// Gives the time, in seconds since 1970, that files written from now on
    /// are marked with.
    pub fn set_now(secs: u64) {
        NOW.fetch_max(secs, Ordering::SeqCst);
    }

    fn nodes() -> MutexGuard<'static, BTreeMap<PathBuf, Node>> {
        NODES.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn data(d: &Mutex<Data>) -> MutexGuard<'_, Data> {
        d.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn not_found() -> io::Error {
        io::Error::from(io::ErrorKind::NotFound)
    }

    fn touch(d: &mut Data) {
        d.modified = NOW.load(Ordering::SeqCst);
    }

    /// The lock on a data folder. There is one process and nothing to lock
    /// against, so it is always taken.
    pub struct DirLock;

    impl DirLock {
        /// Takes the lock.
        pub fn acquire(_dir: impl Into<PathBuf>) -> Result<Self, std::convert::Infallible> {
            Ok(Self)
        }
    }

    /// The driver's header and address-book storage. The node only reads the
    /// headers from it before the driver starts; the page's driver keeps
    /// none, so nothing is stored.
    pub struct FileStorage;

    impl FileStorage {
        /// Storage that holds nothing.
        pub fn new(_dir: impl Into<PathBuf>) -> io::Result<Self> {
            Ok(Self)
        }
    }

    impl openbmt_net_io::Storage for FileStorage {
        type Error = io::Error;

        fn load(&mut self, _key: openbmt_net_io::Key) -> io::Result<Option<Vec<u8>>> {
            Ok(None)
        }

        fn save(&mut self, _key: openbmt_net_io::Key, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }

    /// What a file's metadata says.
    pub struct Metadata {
        len: u64,
        modified: u64,
    }

    impl Metadata {
        /// The size in bytes.
        pub fn len(&self) -> u64 {
            self.len
        }

        /// When the file was last written.
        pub fn modified(&self) -> io::Result<SystemTime> {
            Ok(UNIX_EPOCH + Duration::from_secs(self.modified))
        }
    }

    /// An entry of a directory.
    pub struct DirEntry(OsString);

    impl DirEntry {
        /// The name, without the directory.
        pub fn file_name(&self) -> OsString {
            self.0.clone()
        }
    }

    /// The `std::fs` functions the counter calls.
    pub mod fs {
        use super::*;

        /// Makes the directory and every one above it.
        pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
            let mut map = nodes();
            for p in path.as_ref().ancestors() {
                if !p.as_os_str().is_empty() {
                    map.entry(p.to_path_buf()).or_insert(Node::Dir);
                }
            }
            Ok(())
        }

        /// The bytes of a file.
        pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
            match nodes().get(path.as_ref()) {
                Some(Node::File(d)) => Ok(data(d).bytes.clone()),
                Some(Node::Dir) => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                None => Err(not_found()),
            }
        }

        /// Writes a file whole.
        pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
            let mut f = File::create(path)?;
            f.write_all(bytes.as_ref())
        }

        /// Takes a file away.
        pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
            let mut map = nodes();
            match map.get(path.as_ref()) {
                Some(Node::File(_)) => {
                    map.remove(path.as_ref());
                    Ok(())
                }
                Some(Node::Dir) => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                None => Err(not_found()),
            }
        }

        /// Takes a directory and everything under it away.
        pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
            let path = path.as_ref();
            let mut map = nodes();
            if !map.contains_key(path) {
                return Err(not_found());
            }
            map.retain(|p, _| !p.starts_with(path));
            Ok(())
        }

        /// Moves a file, replacing what is at the new path.
        pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
            let mut map = nodes();
            let node = map.remove(from.as_ref()).ok_or_else(not_found)?;
            map.insert(to.as_ref().to_path_buf(), node);
            Ok(())
        }

        /// The size and time of a file or directory.
        pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
            match nodes().get(path.as_ref()) {
                Some(Node::File(d)) => {
                    let d = data(d);
                    Ok(Metadata {
                        len: d.bytes.len() as u64,
                        modified: d.modified,
                    })
                }
                Some(Node::Dir) => Ok(Metadata {
                    len: 0,
                    modified: 0,
                }),
                None => Err(not_found()),
            }
        }

        /// The names directly inside a directory.
        pub fn read_dir(
            path: impl AsRef<Path>,
        ) -> io::Result<std::vec::IntoIter<io::Result<DirEntry>>> {
            let path = path.as_ref();
            let map = nodes();
            if !matches!(map.get(path), Some(Node::Dir)) {
                return Err(not_found());
            }
            let names: Vec<io::Result<DirEntry>> = map
                .keys()
                .filter(|p| p.parent() == Some(path))
                .filter_map(|p| p.file_name())
                .map(|n| Ok(DirEntry(n.to_os_string())))
                .collect();
            Ok(names.into_iter())
        }
    }

    /// An open file.
    pub struct File {
        data: Arc<Mutex<Data>>,
        pos: u64,
        append: bool,
        write: bool,
    }

    impl File {
        /// Opens a file for reading (or a directory, to flush it).
        pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
            OpenOptions::new().read(true).open(path)
        }

        /// Makes a file, or empties the one there.
        pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
            let f = OpenOptions::new().write(true).create(true).open(path)?;
            f.set_len(0)?;
            Ok(f)
        }

        /// Cuts or extends the file to this size.
        pub fn set_len(&self, size: u64) -> io::Result<()> {
            let mut d = data(&self.data);
            d.bytes.resize(
                usize::try_from(size).map_err(|_| io::ErrorKind::InvalidInput)?,
                0,
            );
            touch(&mut d);
            Ok(())
        }

        /// Nothing is flushed: the file is in memory.
        pub fn sync_all(&self) -> io::Result<()> {
            Ok(())
        }

        /// Nothing is flushed: the file is in memory.
        pub fn sync_data(&self) -> io::Result<()> {
            Ok(())
        }

        /// The size and time of the file.
        pub fn metadata(&self) -> io::Result<Metadata> {
            let d = data(&self.data);
            Ok(Metadata {
                len: d.bytes.len() as u64,
                modified: d.modified,
            })
        }
    }

    impl Read for File {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let d = data(&self.data);
            let start = usize::try_from(self.pos).unwrap_or(usize::MAX);
            let rest = d.bytes.get(start..).unwrap_or_default();
            let n = rest.len().min(out.len());
            if let (Some(dst), Some(src)) = (out.get_mut(..n), rest.get(..n)) {
                dst.copy_from_slice(src);
            }
            self.pos = self.pos.saturating_add(n as u64);
            Ok(n)
        }
    }

    impl Write for File {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.write {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            let mut d = data(&self.data);
            if self.append {
                self.pos = d.bytes.len() as u64;
            }
            let start = usize::try_from(self.pos).map_err(|_| io::ErrorKind::InvalidInput)?;
            let end = start.saturating_add(bytes.len());
            if d.bytes.len() < end {
                d.bytes.resize(end, 0);
            }
            if let Some(dst) = d.bytes.get_mut(start..end) {
                dst.copy_from_slice(bytes);
            }
            touch(&mut d);
            self.pos = end as u64;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Seek for File {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            let len = data(&self.data).bytes.len() as i64;
            let next = match to {
                SeekFrom::Start(n) => i64::try_from(n).map_err(|_| io::ErrorKind::InvalidInput)?,
                SeekFrom::End(n) => len.saturating_add(n),
                SeekFrom::Current(n) => (self.pos as i64).saturating_add(n),
            };
            if next < 0 {
                return Err(io::Error::from(io::ErrorKind::InvalidInput));
            }
            self.pos = next as u64;
            Ok(self.pos)
        }
    }

    /// How a file is opened.
    #[derive(Default, Clone)]
    pub struct OpenOptions {
        write: bool,
        append: bool,
        create: bool,
    }

    impl OpenOptions {
        /// Options that read.
        pub fn new() -> Self {
            Self::default()
        }

        /// Reading is always allowed.
        pub fn read(&mut self, _on: bool) -> &mut Self {
            self
        }

        /// Allow writing.
        pub fn write(&mut self, on: bool) -> &mut Self {
            self.write = on;
            self
        }

        /// Write at the end.
        pub fn append(&mut self, on: bool) -> &mut Self {
            self.append = on;
            self.write |= on;
            self
        }

        /// Make the file if it is not there.
        pub fn create(&mut self, on: bool) -> &mut Self {
            self.create = on;
            self
        }

        /// Open the file.
        pub fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
            let mut map = nodes();
            let data = match map.get(path.as_ref()) {
                Some(Node::File(d)) => d.clone(),
                Some(Node::Dir) => Arc::default(),
                None if self.create => {
                    let d = Arc::new(Mutex::new(Data::default()));
                    touch(&mut data(&d));
                    map.insert(path.as_ref().to_path_buf(), Node::File(d.clone()));
                    d
                }
                None => return Err(not_found()),
            };
            Ok(File {
                data,
                pos: 0,
                append: self.append,
                write: self.write,
            })
        }
    }
}
