use lightning::ln::types::ChannelId;
use lightning::sign::KeysManager;
use lightning::util::logger::Level;
use lightning::util::persist::KVStore;
use lightning_persister::fs_store::FilesystemStore;
use openbmt_lightning_node::bip85::NodeSeed;
use openbmt_lightning_node::device::{DeviceError, DeviceFuture, DeviceStatus, SecondDevice};
use openbmt_lightning_node::logger::Logger;
use openbmt_lightning_node::seal::{open, seal, SealKey};
use openbmt_lightning_node::state::{
    self, Action, CopyOrder, StartFrom, StateError, UpdateNumbers,
};
use openbmt_lightning_node::store::{DeviceView, DualStore};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        let n = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "openbmt-recovery-{label}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &PathBuf {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Backend {
    available: AtomicBool,
    discard_writes: AtomicBool,
    blobs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}
impl Backend {
    fn new() -> Self {
        Self {
            available: AtomicBool::new(true),
            discard_writes: AtomicBool::new(false),
            blobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    fn set_available(&self, yes: bool) {
        self.available.store(yes, Ordering::SeqCst);
    }
    fn set_discard_writes(&self, yes: bool) {
        self.discard_writes.store(yes, Ordering::SeqCst);
    }
}
impl SecondDevice for Backend {
    fn write(&self, name: &str, blob: Vec<u8>) -> DeviceFuture<()> {
        let (name, blobs) = (name.to_owned(), Arc::clone(&self.blobs));
        let available = self.available.load(Ordering::SeqCst);
        let discard = self.discard_writes.load(Ordering::SeqCst);
        Box::pin(async move {
            if !available {
                return Err(DeviceError("injected unavailable".into()));
            }
            if !discard {
                blobs.lock().unwrap().insert(name, blob);
            }
            Ok(())
        })
    }
    fn read(&self, name: &str) -> DeviceFuture<Option<Vec<u8>>> {
        let (name, blobs) = (name.to_owned(), Arc::clone(&self.blobs));
        let available = self.available.load(Ordering::SeqCst);
        Box::pin(async move {
            if !available {
                return Err(DeviceError("injected unavailable".into()));
            }
            Ok(blobs.lock().unwrap().get(&name).cloned())
        })
    }
    fn list(&self) -> DeviceFuture<Vec<String>> {
        let blobs = Arc::clone(&self.blobs);
        let available = self.available.load(Ordering::SeqCst);
        Box::pin(async move {
            if !available {
                return Err(DeviceError("injected unavailable".into()));
            }
            Ok(blobs.lock().unwrap().keys().cloned().collect())
        })
    }
    fn remove(&self, name: &str) -> DeviceFuture<()> {
        let (name, blobs) = (name.to_owned(), Arc::clone(&self.blobs));
        let available = self.available.load(Ordering::SeqCst);
        Box::pin(async move {
            if !available {
                return Err(DeviceError("injected unavailable".into()));
            }
            blobs.lock().unwrap().remove(&name);
            Ok(())
        })
    }
}

struct Rig {
    _dir: TempDir,
    local: Arc<FilesystemStore>,
    device: Arc<Backend>,
    status: Arc<DeviceStatus>,
    store: DualStore,
    view: DeviceView,
}
fn rig(label: &str) -> Rig {
    let dir = TempDir::new(label);
    let local = Arc::new(FilesystemStore::new(dir.path().join("local")));
    let device = Arc::new(Backend::new());
    let status = Arc::new(DeviceStatus::new());
    let key = SealKey::from_seed(&NodeSeed([0x5a; 64])); // fixed test fixture only
    let keys = Arc::new(KeysManager::new(&[0x31; 32], 1, 1, true)); // fixed test fixture only
    let logger = Arc::new(Logger::open(&dir.path().join("test.log"), Level::Debug).unwrap());
    let device_dyn: Arc<dyn SecondDevice> = device.clone();
    let store = DualStore::new(
        Arc::clone(&local),
        Arc::clone(&device_dyn),
        key.clone(),
        Arc::clone(&keys),
        Arc::clone(&status),
        logger,
    );
    let view = DeviceView::new(device_dyn, key.clone(), Arc::clone(&keys));
    Rig {
        _dir: dir,
        local,
        device,
        status,
        store,
        view,
    }
}

#[tokio::test]
async fn strict_write_keeps_old_local_value_and_completion_waits_for_second_device() {
    let r = rig("pending");
    r.store
        .write("monitors", "", "chan", b"old".to_vec())
        .await
        .unwrap();
    r.device.set_available(false);
    let store = r.store.clone();
    let task =
        tokio::spawn(async move { store.write("monitors", "", "chan", b"new".to_vec()).await });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if r.status.snapshot().pending_writes == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!task.is_finished());
    assert_eq!(
        KVStore::read(&*r.local, "monitors", "", "chan")
            .await
            .unwrap(),
        b"old"
    );
    r.device.set_available(true);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        KVStore::read(&*r.local, "monitors", "", "chan")
            .await
            .unwrap(),
        b"new"
    );
    assert_eq!(r.view.read("monitors", "", "chan").await.unwrap(), b"new");
}

#[test]
fn sealed_blob_is_authenticated_but_an_older_valid_blob_under_same_name_replays() {
    let key = SealKey::from_seed(&NodeSeed([0x5a; 64]));
    let name = "monitors~~fixture";
    let older = seal(&key, [0x22; 12], name, b"older valid state"); // fixed test nonce only
    let newer = seal(&key, [0x23; 12], name, b"newer state");
    assert_eq!(open(&key, name, &older).unwrap(), b"older valid state");
    let mut changed = newer.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert!(open(&key, name, &changed).is_err());
    assert!(open(&key, "monitors~~other", &newer).is_err());
}

#[test]
fn update_number_maps_choose_newer_and_reject_divergence_but_identical_maps_go() {
    fn map(xs: &[(u8, u64)]) -> UpdateNumbers {
        xs.iter()
            .map(|(n, v)| (ChannelId::from_bytes([*n; 32]), *v))
            .collect::<BTreeMap<_, _>>()
    }
    let old = map(&[(1, 4), (2, 8)]);
    let newer = map(&[(1, 5), (2, 8)]);
    assert_eq!(state::order(&old, &newer), CopyOrder::DeviceNewer);
    assert_eq!(
        state::action(CopyOrder::DeviceNewer, StartFrom::Newer),
        Ok(Action::PullFromDevice)
    );
    assert_eq!(
        state::action(CopyOrder::DeviceNewer, StartFrom::Local),
        Err(StateError::OlderCopy(CopyOrder::DeviceNewer))
    );
    assert_eq!(state::order(&old, &old), CopyOrder::Same);
    assert_eq!(
        state::action(CopyOrder::Same, StartFrom::Newer),
        Ok(Action::Go)
    );
    assert_eq!(
        state::order(&map(&[(1, 6)]), &map(&[(2, 9)])),
        CopyOrder::Diverged
    );
    assert_eq!(
        state::action(CopyOrder::Diverged, StartFrom::Newer),
        Err(StateError::Diverged)
    );
}

#[tokio::test]
async fn backend_ack_without_durability_allows_completion_but_fresh_view_finds_nothing() {
    let r = rig("ack-without-durability");
    r.device.set_discard_writes(true); // violates SecondDevice::write durability contract
    r.store
        .write("manager", "", "state", b"committed locally".to_vec())
        .await
        .unwrap();
    assert_eq!(
        KVStore::read(&*r.local, "manager", "", "state")
            .await
            .unwrap(),
        b"committed locally"
    );
    assert!(r.view.read("manager", "", "state").await.is_err());
}
