//! Fixed keys and helpers shared by the tests.

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;

use openbmt_core::identity::{DeviceId, Identity};

use crate::seal::{seal, Kind, Role};

/// A numbered identity.
pub fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

pub fn owner() -> Identity {
    id(1)
}

pub fn counter() -> Identity {
    id(2)
}

pub fn the_box() -> Identity {
    id(3)
}

pub fn stranger() -> Identity {
    id(4)
}

/// The roles of the three devices, as a device list would give them.
pub fn roles() -> HashMap<DeviceId, Role> {
    HashMap::from([
        (owner().device_id(), Role::Owner),
        (counter().device_id(), Role::Counter),
        (the_box().device_id(), Role::Box),
    ])
}

pub fn allowed(d: &DeviceId) -> Option<Role> {
    roles().get(d).copied()
}

pub fn none_taken(_: &DeviceId) -> u64 {
    0
}

/// "Randomness" that counts: each call gives a different valid secret.
pub struct Counting(pub u8);

impl Counting {
    pub fn fill(&mut self, buf: &mut [u8; 32]) -> Result<(), Infallible> {
        self.0 = self.0.wrapping_add(1);
        *buf = [self.0 | 0x40; 32];
        Ok(())
    }
}

/// Seal from the owner to the counter with counting randomness.
pub fn owner_to_counter(number: u64, kind: Kind, content: &[u8]) -> Vec<u8> {
    let mut rng = Counting(0);
    seal(
        &owner(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        number,
        kind,
        content,
        &mut |b| rng.fill(b),
    )
    .unwrap()
}

/// A directory under the system temp dir that is removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let mut p = std::env::temp_dir();
        p.push(format!("openbmt-link-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
