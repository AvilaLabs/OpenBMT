#![no_main]

use std::io;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use openbmt_box::{BoxCore, MemFiles};
use openbmt_core::batch::PinnedRoots;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::{Settings, SignedSettings};

const NOW: u64 = 1_000_000;
const BOX_BYTE: u8 = 0x77;

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

fn fixed_random(buf: &mut [u8; 32]) -> io::Result<()> {
    *buf = [BOX_BYTE; 32];
    Ok(())
}

/// A paired Box (settings version 1), and valid settings of version 2 that
/// list the Box and of version 3 that do not, so that mutated bytes get
/// past the signatures now and then.
struct Fixture {
    paired: MemFiles,
    listed: Vec<u8>,
    unlisted: Vec<u8>,
}

fn settings_with(version: u32, boxes: &[DeviceId], devices: &[DeviceId]) -> Vec<u8> {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    let counters = [id(3).device_id(), id(4).device_id()];
    s.devices = counters.iter().chain(devices).copied().collect();
    s.counters = counters.into_iter().map(|c| (c, 2_500u16)).collect();
    s.boxes = boxes.iter().copied().collect();
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
    .to_bytes()
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut r = [0u8; 17];
        for (i, b) in r.iter_mut().enumerate() {
            *b = 5u8.wrapping_add(i as u8).wrapping_mul(29);
        }
        let inv = Invitation::from_random(&r);
        let mut core = BoxCore::open(MemFiles::new(), &mut fixed_random).unwrap();
        core.enter_invitation(&inv.to_text(), NOW, [0x31; 32])
            .unwrap();
        let claim = Claim::from_bytes(&core.claim(NOW + 1).unwrap()).unwrap();
        let roots = PinnedRoots::new(id(1).device_id(), id(2).device_id()).unwrap();
        let welcome = Welcome::make(&inv, &claim, roots).to_bytes();
        let me = core.device_id();
        let first = settings_with(1, &[me], &[me]);
        core.take_welcome(&welcome, &first, NOW + 2).unwrap();
        Fixture {
            paired: core.into_files(),
            listed: settings_with(2, &[me], &[me]),
            unlisted: settings_with(3, &[], &[]),
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let f = fixture();
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    // The first byte picks the bytes handed in: the input as it is, valid
    // settings with the input written over their tail from a place the byte
    // gives, or valid settings with one bit changed.
    let valid = if mode & 4 == 4 { &f.unlisted } else { &f.listed };
    let settings: Vec<u8> = match mode % 3 {
        0 => rest.to_vec(),
        1 => {
            let keep = usize::from(mode >> 3).min(valid.len());
            let mut v = valid[..keep].to_vec();
            v.extend_from_slice(rest);
            v
        }
        _ => {
            let mut v = valid.clone();
            if let Some(&at) = rest.first() {
                let at = (usize::from(at) | usize::from(*rest.get(1).unwrap_or(&0)) << 8) % v.len();
                v[at] ^= 1 << (mode >> 3 & 7);
            }
            v
        }
    };
    let mut core = BoxCore::open(f.paired.clone(), &mut fixed_random).unwrap();
    let _ = core.take_settings(&settings);
    // Taken or refused, what is on disk reads back.
    BoxCore::open(core.into_files(), &mut fixed_random).unwrap();
});
