//! The program that runs as process 1 on the stick (SK-10): it mounts what it
//! needs, loads the keyboard modules, shows the screens of the rules and
//! turns the computer off.

mod sys;

use std::time::Duration;

use openbmt_stick::app::{App, Chance, Outcome};
use openbmt_stick::screen::Screen as Page;

/// How long to wait for the frame buffer before giving up.
const FRAME_BUFFER_WAIT: Duration = Duration::from_secs(30);

fn show(display: &mut sys::Screen, page: &Page) {
    display.show(page);
    #[cfg(feature = "serial-echo")]
    sys::serial_echo(page);
}

fn main() {
    // Whatever goes wrong, the computer is turned off, never left running.
    std::panic::set_hook(Box::new(|_| sys::power_off()));
    sys::mount_all();
    sys::load_modules();
    // SK-20: wait until the kernel says its generator is ready.
    let mut ready = sys::KernelChance.kernel_bytes();
    ready.fill(0);
    let facts = sys::read_facts();
    let mut display = match sys::Screen::open(FRAME_BUFFER_WAIT) {
        Ok(d) => d,
        Err(_) => sys::power_off(),
    };
    let mut keys = sys::Keyboards::new();
    let mut app = App::new(facts, sys::KernelChance);
    loop {
        show(&mut display, &app.screen());
        let key = keys.next_key();
        if app.key(key) == Outcome::TurnOff {
            break;
        }
    }
    sys::power_off();
}
