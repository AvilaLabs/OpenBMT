//! Fixed keys, a relay in memory, scratch directories and couriers on them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::{Chain, Record, RecordKind};
use openbmt_link::Role;

use crate::{Courier, FirstStart, MemoryPost, MemoryRelay};

pub fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

pub fn owner() -> DeviceId {
    id(1).device_id()
}

pub fn counter() -> DeviceId {
    id(2).device_id()
}

pub fn stranger() -> DeviceId {
    id(4).device_id()
}

/// A relay whose members are the owner (1), the counter (2), the box (3)
/// and a stranger (4): a member of the relay is not a device of the shop.
pub fn relay() -> MemoryRelay {
    let members: Vec<DeviceId> = (1..=4).map(|n| id(n).device_id()).collect();
    MemoryRelay::new(id(99), &members).unwrap()
}

/// A directory that is removed when it goes out of scope.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "openbmt-courier-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn table(peers: &[(u8, Role)]) -> BTreeMap<DeviceId, Role> {
    peers
        .iter()
        .map(|(n, role)| (id(*n).device_id(), *role))
        .collect()
}

/// The owner's courier: it knows the counter.
pub fn owner_courier(relay: &MemoryRelay, dir: &Path, first: FirstStart) -> Courier<MemoryPost> {
    Courier::start_with(
        relay.post(),
        id(1),
        Role::Owner,
        table(&[(2, Role::Counter)]),
        dir,
        first,
    )
    .unwrap()
}

/// The counter's courier: it knows the owner.
pub fn counter_courier(relay: &MemoryRelay, dir: &Path, first: FirstStart) -> Courier<MemoryPost> {
    Courier::start_with(
        relay.post(),
        id(2),
        Role::Counter,
        table(&[(1, Role::Owner)]),
        dir,
        first,
    )
    .unwrap()
}

/// `n` records signed by the counter, with a body of `body` bytes each.
pub fn records(n: usize, body: usize) -> Vec<Record> {
    let c = id(2);
    let mut chain = Chain::new();
    for i in 0..n {
        let mut b = vec![(i % 251) as u8; body];
        if let Some(first) = b.first_mut() {
            *first = (i >> 8) as u8;
        }
        chain.append(&c, RecordKind::Sale, b).unwrap();
    }
    chain.records().to_vec()
}
