//! The program that runs as process 1 on the stick (SK-10): it mounts what it
//! needs, loads the keyboard modules, shows the screens of the rules and
//! turns the computer off.

// The test build reads frames from the serial port instead (SK-58).
#[cfg(not(feature = "serial-frames"))]
mod camera;
mod display;
mod sys;

use std::time::{Duration, Instant};

use openbmt_stick::app::{App, Chance, Outcome};
use openbmt_stick::screen::Screen as Page;

/// How long to wait for the frame buffer before giving up.
const FRAME_BUFFER_WAIT: Duration = Duration::from_secs(30);
/// How often to look for a camera that has not enumerated yet (SK-52).
#[cfg(not(feature = "serial-frames"))]
const CAMERA_LOOK: Duration = Duration::from_millis(500);
/// How long to wait for a key while the screen moves or frames come in.
const BUSY_POLL_MS: i32 = 30;

fn show(display: &mut display::Screen, page: &Page) {
    display.show(page);
    #[cfg(feature = "serial-echo")]
    sys::serial_echo(page);
}

fn main() {
    // Whatever goes wrong, the computer is turned off, never left running.
    std::panic::set_hook(Box::new(|_| sys::power_off()));
    sys::mount_all();
    sys::load_modules();
    #[cfg(feature = "serial-echo")]
    sys::serial_modules("start");
    // SK-20: wait until the kernel says its generator is ready.
    let mut ready = sys::KernelChance.kernel_bytes();
    ready.fill(0);
    let facts = sys::read_facts();
    let mut display = match display::Screen::open(FRAME_BUFFER_WAIT) {
        Ok(d) => d,
        Err(_) => sys::power_off(),
    };
    let mut keys = sys::Keyboards::new();
    let mut app = App::new(facts, sys::KernelChance);
    #[cfg(not(feature = "serial-frames"))]
    let mut camera: Option<camera::Camera> = None;
    #[cfg(not(feature = "serial-frames"))]
    let mut looked = Instant::now();
    #[cfg(feature = "serial-frames")]
    let mut serial = sys::SerialFrames::open();
    let mut last = Instant::now();
    show(&mut display, &app.screen());
    loop {
        let moving = app.animated();
        let key = if moving {
            keys.poll_key(BUSY_POLL_MS)
        } else {
            Some(keys.next_key())
        };
        let mut changed = false;
        if let Some(key) = key {
            match app.key(key) {
                Outcome::TurnOff => break,
                Outcome::LoadCamera => {
                    // SK-51: the camera modules, only now.
                    sys::load_camera_modules();
                    #[cfg(feature = "serial-echo")]
                    sys::serial_modules("camera");
                    // SK-58: the test build reads frames from the serial port.
                    #[cfg(feature = "serial-frames")]
                    app.camera_found();
                }
                Outcome::Continue => {}
            }
            changed = true;
        }
        if app.wants_frames() {
            #[cfg(feature = "serial-frames")]
            for text in serial.texts() {
                changed |= app.frame_text(&text);
            }
            #[cfg(not(feature = "serial-frames"))]
            {
                if camera.is_none() && looked.elapsed() >= CAMERA_LOOK {
                    looked = Instant::now();
                    camera = camera::Camera::open_first();
                    if camera.is_some() {
                        app.camera_found();
                        changed = true;
                    }
                }
                if let Some(c) = camera.as_mut() {
                    if let Some(luma) = c.next_frame() {
                        let (w, h) = c.size();
                        changed |= app.frame_luma(w, h, &luma);
                    }
                }
            }
        } else {
            // Not reading: the camera is closed.
            #[cfg(not(feature = "serial-frames"))]
            {
                camera = None;
            }
        }
        let now = Instant::now();
        // Time counts only while the screen moves, so a long wait for a key
        // is not taken as time spent looking for a camera.
        if moving && app.animated() {
            changed |= app.tick(now.duration_since(last).as_millis() as u64);
        }
        last = now;
        if changed {
            show(&mut display, &app.screen());
        }
    }
    sys::power_off();
}
