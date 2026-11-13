//! Which copy of the state is the newer, and bringing the other to it (SL-25).
//!
//! On a start the node reads the update numbers of every channel monitor in
//! both copies, the counter's own and the second device's. It starts only
//! from the newer one. A copy behind the other is never used: it is brought to
//! the newer before anything else, and a start that is told to use the older
//! is refused. Two copies that each hold something the other lacks are
//! refused too, as the node cannot say which is the state.
//!
//! The channel manager's blob has no update number and is not compared: a
//! manager older than its monitors is a case the library reads (the monitors
//! are the truth), and a copy brought over is brought whole.

use crate::chain::{CoreBroadcaster, CoreFees};
use crate::logger::Logger;
use crate::store::{device_name, split_device_name, DeviceView};
use lightning::io;
use lightning::ln::types::ChannelId;
use lightning::sign::KeysManager;
use lightning::util::native_async::FutureSpawner;
use lightning::util::persist::{KVStore, MonitorUpdatingPersisterAsync};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::Path;
use std::sync::Arc;

/// The latest update number of every channel monitor in one copy.
pub type UpdateNumbers = BTreeMap<ChannelId, u64>;

/// How the two copies stand to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOrder {
    /// Neither holds a channel monitor.
    BothEmpty,
    /// Every monitor is at the same number in both.
    Same,
    /// The counter's copy is ahead (the second device is behind or new).
    LocalNewer,
    /// The second device's copy is ahead (the counter's is behind or gone).
    DeviceNewer,
    /// Each is ahead of the other somewhere.
    Diverged,
}

/// Which copy a start may use (SL-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartFrom {
    /// The newer of the two: the other is brought up to it first.
    Newer,
    /// The counter's copy. Refused if the second device's is newer.
    Local,
    /// The second device's copy. Refused if the counter's is newer.
    Device,
}

/// Why a start does not go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    /// The copy asked for is older than the other (SL-25).
    OlderCopy(CopyOrder),
    /// The copies are each ahead of the other somewhere (SL-25).
    Diverged,
}

/// How the two copies stand: a missing monitor counts as behind any number.
pub fn order(local: &UpdateNumbers, device: &UpdateNumbers) -> CopyOrder {
    if local.is_empty() && device.is_empty() {
        return CopyOrder::BothEmpty;
    }
    let (mut local_ahead, mut device_ahead) = (false, false);
    let ids: BTreeSet<&ChannelId> = local.keys().chain(device.keys()).collect();
    for id in ids {
        match (local.get(id), device.get(id)) {
            (Some(l), Some(d)) if l > d => local_ahead = true,
            (Some(l), Some(d)) if l < d => device_ahead = true,
            (Some(_), None) => local_ahead = true,
            (None, Some(_)) => device_ahead = true,
            _ => {}
        }
    }
    match (local_ahead, device_ahead) {
        (false, false) => CopyOrder::Same,
        (true, false) => CopyOrder::LocalNewer,
        (false, true) => CopyOrder::DeviceNewer,
        (true, true) => CopyOrder::Diverged,
    }
}

/// What a start does about the order: nothing, bring the device up, bring the
/// counter's copy up, or refuse (SL-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The copies agree.
    Go,
    /// Copy the counter's state to the second device, then go.
    PushToDevice,
    /// Copy the second device's state here, then go.
    PullFromDevice,
}

/// The action for an order and a choice of copy.
pub fn action(found: CopyOrder, from: StartFrom) -> Result<Action, StateError> {
    match (found, from) {
        (CopyOrder::Diverged, _) => Err(StateError::Diverged),
        (CopyOrder::BothEmpty | CopyOrder::Same, _) => Ok(Action::Go),
        (CopyOrder::LocalNewer, StartFrom::Newer | StartFrom::Local) => Ok(Action::PushToDevice),
        (CopyOrder::DeviceNewer, StartFrom::Newer | StartFrom::Device) => Ok(Action::PullFromDevice),
        (found, _) => Err(StateError::OlderCopy(found)),
    }
}

/// What reading the monitors of a copy needs.
#[derive(Clone)]
pub struct ReadEnv {
    /// The keys.
    pub keys: Arc<KeysManager>,
    /// The broadcaster the monitors are read with.
    pub broadcaster: Arc<CoreBroadcaster>,
    /// The fee estimates the monitors are read with.
    pub fees: Arc<CoreFees>,
    /// The log.
    pub logger: Arc<Logger>,
}

/// A spawner that never spawns: reading monitors writes nothing.
struct NoSpawn;

impl FutureSpawner for NoSpawn {
    fn spawn<T: Future<Output = ()> + Send + 'static>(&self, _future: T) {
        unreachable!("reading a copy of the state spawns nothing")
    }
}

/// The latest update number of every channel monitor in `store`, the monitor
/// with its stored updates applied.
pub async fn update_numbers<K: KVStore + Send + Sync + 'static>(
    store: Arc<K>, env: &ReadEnv,
) -> Result<UpdateNumbers, io::Error> {
    let persister = MonitorUpdatingPersisterAsync::new(
        store,
        NoSpawn,
        Arc::clone(&env.logger),
        1,
        Arc::clone(&env.keys),
        Arc::clone(&env.keys),
        Arc::clone(&env.broadcaster),
        Arc::clone(&env.fees),
    );
    let monitors = persister.read_all_channel_monitors_with_updates().await?;
    Ok(monitors.iter().map(|(_, m)| (m.channel_id(), m.get_latest_update_id())).collect())
}

/// Every key in a directory store, as (primary, secondary, key).
pub fn local_keys(dir: &Path) -> std::io::Result<Vec<(String, String, String)>> {
    fn visible(name: &str) -> bool {
        !name.starts_with('.') && !name.ends_with(".tmp")
    }
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for top in std::fs::read_dir(dir)? {
        let top = top?;
        let tname = top.file_name().to_string_lossy().into_owned();
        if !visible(&tname) {
            continue;
        }
        if top.file_type()?.is_file() {
            out.push((String::new(), String::new(), tname));
            continue;
        }
        for second in std::fs::read_dir(top.path())? {
            let second = second?;
            let sname = second.file_name().to_string_lossy().into_owned();
            if !visible(&sname) {
                continue;
            }
            if second.file_type()?.is_file() {
                out.push((tname.clone(), String::new(), sname));
                continue;
            }
            for leaf in std::fs::read_dir(second.path())? {
                let leaf = leaf?;
                let lname = leaf.file_name().to_string_lossy().into_owned();
                if visible(&lname) && leaf.file_type()?.is_file() {
                    out.push((tname.clone(), sname.clone(), lname));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Makes the second device hold exactly what the counter's copy holds.
pub async fn push<K: KVStore>(
    local_dir: &Path, local: &K, device: &DeviceView,
) -> Result<usize, io::Error> {
    let keys = local_keys(local_dir)?;
    let mut names = BTreeSet::new();
    for (p, s, k) in &keys {
        let blob = local.read(p, s, k).await?;
        device.write(p, s, k, blob).await?;
        names.insert(device_name(p, s, k));
    }
    for name in device.names().await? {
        if !names.contains(&name) {
            if let Some((p, s, k)) = split_device_name(&name) {
                device.remove(p, s, k, false).await?;
            }
        }
    }
    Ok(keys.len())
}

/// Makes the counter's copy hold exactly what the second device holds.
pub async fn pull<K: KVStore>(
    local_dir: &Path, local: &K, device: &DeviceView,
) -> Result<usize, io::Error> {
    let names = device.names().await?;
    let mut wanted = BTreeSet::new();
    for name in &names {
        let Some((p, s, k)) = split_device_name(name) else { continue };
        let blob = device.read(p, s, k).await?;
        local.write(p, s, k, blob).await?;
        wanted.insert(name.clone());
    }
    for (p, s, k) in local_keys(local_dir)? {
        if !wanted.contains(&device_name(&p, &s, &k)) {
            local.remove(&p, &s, &k, false).await?;
        }
    }
    Ok(names.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> ChannelId {
        ChannelId::from_bytes([n; 32])
    }

    fn nums(v: &[(u8, u64)]) -> UpdateNumbers {
        v.iter().map(|(n, u)| (id(*n), *u)).collect()
    }

    #[test]
    fn sl_25_the_order_of_two_copies() {
        let a = nums(&[(1, 5), (2, 7)]);
        assert_eq!(order(&nums(&[]), &nums(&[])), CopyOrder::BothEmpty);
        assert_eq!(order(&a, &a), CopyOrder::Same);
        assert_eq!(order(&nums(&[(1, 6), (2, 7)]), &a), CopyOrder::LocalNewer);
        assert_eq!(order(&a, &nums(&[(1, 5), (2, 9)])), CopyOrder::DeviceNewer);
        assert_eq!(order(&nums(&[(1, 6), (2, 7)]), &nums(&[(1, 5), (2, 9)])), CopyOrder::Diverged);
    }

    #[test]
    fn sl_25_a_copy_that_lacks_a_monitor_is_behind() {
        let a = nums(&[(1, 5)]);
        assert_eq!(order(&a, &nums(&[])), CopyOrder::LocalNewer);
        assert_eq!(order(&nums(&[]), &a), CopyOrder::DeviceNewer);
        assert_eq!(order(&nums(&[(1, 0)]), &nums(&[])), CopyOrder::LocalNewer);
        assert_eq!(order(&nums(&[(1, 5)]), &nums(&[(2, 1)])), CopyOrder::Diverged);
    }

    #[test]
    fn sl_25_a_start_from_the_older_copy_is_refused() {
        use Action::*;
        use CopyOrder::*;
        assert_eq!(action(Same, StartFrom::Newer), Ok(Go));
        assert_eq!(action(BothEmpty, StartFrom::Local), Ok(Go));
        assert_eq!(action(LocalNewer, StartFrom::Newer), Ok(PushToDevice));
        assert_eq!(action(LocalNewer, StartFrom::Local), Ok(PushToDevice));
        assert_eq!(action(LocalNewer, StartFrom::Device), Err(StateError::OlderCopy(LocalNewer)));
        assert_eq!(action(DeviceNewer, StartFrom::Newer), Ok(PullFromDevice));
        assert_eq!(action(DeviceNewer, StartFrom::Device), Ok(PullFromDevice));
        assert_eq!(action(DeviceNewer, StartFrom::Local), Err(StateError::OlderCopy(DeviceNewer)));
        for from in [StartFrom::Newer, StartFrom::Local, StartFrom::Device] {
            assert_eq!(action(Diverged, from), Err(StateError::Diverged));
        }
    }

    #[test]
    fn sl_25_the_keys_of_a_directory_store_are_found_at_every_depth() {
        let dir = crate::testutil::temp_dir("local-keys");
        std::fs::write(dir.join("manager"), b"m").unwrap();
        std::fs::create_dir_all(dir.join("monitors")).unwrap();
        std::fs::write(dir.join("monitors/abc_0"), b"x").unwrap();
        std::fs::create_dir_all(dir.join("monitor_updates/abc_0")).unwrap();
        std::fs::write(dir.join("monitor_updates/abc_0/00000000000000000001"), b"u").unwrap();
        std::fs::write(dir.join("monitors/.abc.tmp"), b"t").unwrap();
        let keys = local_keys(&dir).unwrap();
        assert_eq!(
            keys,
            vec![
                (String::new(), String::new(), "manager".to_string()),
                ("monitor_updates".into(), "abc_0".into(), "00000000000000000001".into()),
                ("monitors".into(), String::new(), "abc_0".into()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
