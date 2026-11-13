#![no_main]

use std::io;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use openbmt_box::{BoxCore, BoxStatus, MemFiles};
use openbmt_core::batch::PinnedRoots;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::Identity;
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

/// A Box that was given an invitation and made its claim, and the welcome
/// and settings that the Key app and the owner phone make for that claim.
struct Fixture {
    welcome: Vec<u8>,
    settings: Vec<u8>,
    /// BX-18: the welcome of a shop with its own wallet and settings signed
    /// by the owner root alone.
    own_welcome: Vec<u8>,
    own_settings: Vec<u8>,
}

fn invitation() -> Invitation {
    let mut r = [0u8; 17];
    for (i, b) in r.iter_mut().enumerate() {
        *b = 5u8.wrapping_add(i as u8).wrapping_mul(29);
    }
    Invitation::from_random(&r)
}

fn waiting_core() -> (BoxCore<MemFiles>, Vec<u8>) {
    let mut core = BoxCore::open(MemFiles::new(), &mut fixed_random).unwrap();
    core.enter_invitation(&invitation().to_text(), NOW, [0x31; 32])
        .unwrap();
    let claim = core.claim(NOW + 1).unwrap();
    (core, claim)
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let (core, claim_bytes) = waiting_core();
        let roots = PinnedRoots::new(id(1).device_id(), id(2).device_id()).unwrap();
        let claim = Claim::from_bytes(&claim_bytes).unwrap();
        let welcome = Welcome::make(&invitation(), &claim, roots).to_bytes();
        let mut s = Settings::defaults(1_000);
        s.version = 1;
        let counters = [id(3).device_id(), id(4).device_id()];
        s.devices = [counters[0], counters[1], core.device_id()]
            .into_iter()
            .collect();
        s.counters = counters.into_iter().map(|c| (c, 2_500u16)).collect();
        s.boxes = [core.device_id()].into_iter().collect();
        let own_welcome = Welcome::make_own(&invitation(), &claim, id(1).device_id()).to_bytes();
        let own_settings = SignedSettings {
            owner_sig: s.sign(&id(1)),
            approver_sig: None,
            settings: s.clone(),
        }
        .to_bytes();
        let settings = SignedSettings {
            owner_sig: s.sign(&id(1)),
            approver_sig: Some(s.sign(&id(2))),
            settings: s,
        }
        .to_bytes();
        Fixture {
            welcome,
            settings,
            own_welcome,
            own_settings,
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let f = fixture();
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    // The rest is cut in two at a place the first byte gives: the welcome
    // and the settings. Bit 0 swaps the welcome for the valid one and bit 1
    // the settings, so that each is tried with the other right.
    // Bit 7 takes the valid pair of a shop with its own wallet instead.
    let cut = usize::from((mode >> 2) & 0x1f).min(rest.len());
    let (w, s) = rest.split_at(cut);
    let own = mode & 0x80 != 0;
    let (good_welcome, good_settings) = if own {
        (&f.own_welcome, &f.own_settings)
    } else {
        (&f.welcome, &f.settings)
    };
    let welcome: &[u8] = if mode & 1 == 1 { good_welcome } else { w };
    let settings: &[u8] = if mode & 2 == 2 { good_settings } else { s };
    let (mut core, _) = waiting_core();
    let taken = core.take_welcome(welcome, settings, NOW + 2);
    if taken.is_ok() {
        assert!(matches!(
            core.status(NOW + 3),
            BoxStatus::Paired | BoxStatus::TurnedOff
        ));
    }
    // Taken or refused, what is on disk reads back.
    BoxCore::open(core.into_files(), &mut fixed_random).unwrap();
});
