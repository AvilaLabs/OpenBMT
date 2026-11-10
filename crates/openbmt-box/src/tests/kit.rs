//! Fixed keys and helpers shared by the tests: the parts of the Key app and
//! the owner phone that pairing needs, played with the core functions.

use std::io;
use std::path::PathBuf;

use openbmt_core::batch::PinnedRoots;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::{Settings, SignedSettings};

use crate::files::MemFiles;
use crate::state::BoxCore;

/// A numbered identity.
pub fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

pub fn owner() -> Identity {
    id(1)
}

pub fn approver() -> Identity {
    id(2)
}

pub fn counter1() -> Identity {
    id(3)
}

pub fn counter2() -> Identity {
    id(4)
}

pub fn stranger() -> Identity {
    id(9)
}

/// The secret byte the Box's "randomness" gives: its key is `id(BOX_BYTE)`.
pub const BOX_BYTE: u8 = 0x77;

pub fn the_box() -> Identity {
    id(BOX_BYTE)
}

pub fn roots() -> PinnedRoots {
    PinnedRoots::new(owner().device_id(), approver().device_id()).unwrap()
}

/// Randomness that always gives the same valid secret.
pub fn fixed_random(buf: &mut [u8; 32]) -> io::Result<()> {
    *buf = [BOX_BYTE; 32];
    Ok(())
}

pub fn fresh_core() -> BoxCore<MemFiles> {
    BoxCore::open(MemFiles::new(), &mut fixed_random).unwrap()
}

pub fn reopen(core: BoxCore<MemFiles>) -> Result<BoxCore<MemFiles>, crate::state::BoxError> {
    BoxCore::open(core.into_files(), &mut fixed_random)
}

pub const NOW: u64 = 1_000_000;

/// An invitation made from a fixed seed.
pub fn invitation(seed: u8) -> Invitation {
    let mut r = [0u8; 17];
    for (i, b) in r.iter_mut().enumerate() {
        *b = seed.wrapping_add(i as u8).wrapping_mul(29);
    }
    Invitation::from_random(&r)
}

/// The Box is given an invitation at `NOW` with a nonce made from `seed`.
pub fn enter(core: &mut BoxCore<MemFiles>, inv: &Invitation, seed: u8) {
    core.enter_invitation(&inv.to_text(), NOW, [seed; 32])
        .unwrap();
}

/// Settings with two counters and the Box (version `version`), signed by
/// both roots, listing `box_key` as a Box.
pub fn settings_with(version: u32, box_key: &DeviceId) -> Settings {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    s.devices = [counter1().device_id(), counter2().device_id(), *box_key]
        .into_iter()
        .collect();
    s.counters = [
        (counter1().device_id(), 2_500u16),
        (counter2().device_id(), 2_500u16),
    ]
    .into_iter()
    .collect();
    s.boxes = [*box_key].into_iter().collect();
    s
}

pub fn sign_both(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&owner()),
        approver_sig: Some(s.sign(&approver())),
        settings: s,
    }
}

pub fn sign_owner(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&owner()),
        approver_sig: None,
        settings: s,
    }
}

/// The welcome the Key app makes for the claim it was given, with the
/// roots of `roots()`.
pub fn welcome_bytes(inv: &Invitation, claim_bytes: &[u8]) -> Vec<u8> {
    let claim = Claim::from_bytes(claim_bytes).unwrap();
    assert!(claim.verify(inv));
    Welcome::make(inv, &claim, roots()).to_bytes()
}

/// Pair a fresh Box in memory with settings version 1.
pub fn paired_core() -> (BoxCore<MemFiles>, Invitation) {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = core.claim(NOW + 1).unwrap();
    let w = welcome_bytes(&inv, &claim);
    let s = sign_both(settings_with(1, &core.device_id())).to_bytes();
    core.take_welcome(&w, &s, NOW + 2).unwrap();
    (core, inv)
}

/// A directory under the scratch area that is removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let base = std::env::var_os("OPENBMT_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = base.join(format!("openbmt-box-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
