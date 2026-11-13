//! The program's ties to the machine: mounts, modules, the frame buffer, the
//! keyboards, the kernel's random bytes, the serial echo and power off. This
//! is the only part that uses unsafe code; each use says why it is sound.
//! Nothing here opens a network connection or writes to a disk (SK-12,
//! SK-13).

use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bitcoin::hashes::{sha256, Hash};
use openbmt_stick::app::{Chance, Facts};
use openbmt_stick::keys::{key_from_event, Key, EVENT_SIZE};
use openbmt_stick::mix::KERNEL_BYTES;
use openbmt_stick::mounts::MOUNTS;

fn cstr(s: &str) -> io::Result<CString> {
    CString::new(s).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// SK-11: the mounts of `MOUNTS`. A mount that fails (no EFI variables on a
/// BIOS start, say) is skipped; what it would have shown reads as unknown.
pub fn mount_all() {
    for m in MOUNTS {
        let (Ok(src), Ok(target), Ok(fstype)) = (cstr(m.source), cstr(m.target), cstr(m.fstype))
        else {
            continue;
        };
        let flags = if m.read_only { libc::MS_RDONLY } else { 0 };
        // SAFETY: the three strings are NUL-terminated and outlive the call;
        // the data pointer is null, which these file systems accept.
        let _ = unsafe {
            libc::mount(
                src.as_ptr(),
                target.as_ptr(),
                fstype.as_ptr(),
                flags,
                std::ptr::null(),
            )
        };
    }
}

fn release() -> Option<String> {
    // SAFETY: `uname` fills the struct we hand it; a zeroed utsname is valid.
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: `u` is a valid, writable utsname.
    if unsafe { libc::uname(&mut u) } != 0 {
        return None;
    }
    let bytes: Vec<u8> = u
        .release
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8(bytes).ok()
}

fn module_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            module_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// SK-04: loads every module file in `/lib/modules/<release>` (the files of
/// `stick/modules.list`, signed by the distribution, unchanged). A module
/// that needs another waits for it: the pass repeats while any load.
pub fn load_modules() {
    let Some(rel) = release() else { return };
    let mut pending = Vec::new();
    module_files(&Path::new("/lib/modules").join(rel), &mut pending);
    pending.sort();
    loop {
        let before = pending.len();
        pending.retain(|p| !load_one(p));
        if pending.is_empty() || pending.len() == before {
            break;
        }
    }
}

/// True when the module is loaded now (or was already).
fn load_one(path: &Path) -> bool {
    let Ok(f) = File::open(path) else { return true };
    // MODULE_INIT_COMPRESSED_FILE: the kernel decompresses `.ko.zst` itself.
    let compressed = path.extension().is_some_and(|e| e == "zst");
    let flags: libc::c_long = if compressed { 4 } else { 0 };
    let Ok(params) = cstr("") else { return true };
    // SAFETY: a plain system call with an open file descriptor, an empty,
    // NUL-terminated parameter string and the flags above.
    let r = unsafe {
        libc::syscall(
            libc::SYS_finit_module,
            f.as_raw_fd(),
            params.as_ptr(),
            flags,
        )
    };
    r == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST)
}

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// SK-06, SK-12, SK-31: what the first screen shows.
pub fn read_facts() -> Facts {
    let secure_boot =
        fs::read("/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c")
            .ok()
            .and_then(|b| {
                if b.len() == 5 {
                    b.last().copied()
                } else {
                    None
                }
            })
            .map_or("?".to_string(), |v| v.to_string());
    let lockdown = read_trim("/sys/kernel/security/lockdown")
        .and_then(|s| {
            let a = s.find('[')? + 1;
            let b = s.find(']')?;
            s.get(a..b).map(str::to_string)
        })
        .unwrap_or_else(|| "?".to_string());
    let mut interfaces: Vec<String> = fs::read_dir("/sys/class/net")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    interfaces.sort();
    let version = format!(
        "{} {}",
        option_env!("CARGO_PKG_VERSION").unwrap_or("0"),
        option_env!("OPENBMT_STICK_COMMIT").unwrap_or("dev")
    );
    Facts {
        secure_boot,
        lockdown,
        interfaces,
        version,
        initrd_sha256: initrd_hash(),
    }
}

/// SK-31: the SHA-256 of `initrd.img` as the kernel loaded it, read from
/// memory (the kernel keeps it with `retain_initrd` and shows it here).
fn initrd_hash() -> String {
    let Ok(mut f) = File::open("/sys/firmware/initrd") else {
        return "?".to_string();
    };
    let mut engine = sha256::HashEngine::default();
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        match f.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => bitcoin::hashes::HashEngine::input(&mut engine, &chunk[..n]),
            Err(_) => return "?".to_string(),
        }
    }
    sha256::Hash::from_engine(engine).to_string()
}

/// SK-20: 32 bytes from `getrandom`, which blocks until the kernel's
/// generator is ready.
pub struct KernelChance;

impl Chance for KernelChance {
    fn kernel_bytes(&mut self) -> [u8; KERNEL_BYTES] {
        let mut out = [0u8; KERNEL_BYTES];
        let mut have = 0;
        while have < KERNEL_BYTES {
            // SAFETY: the pointer and length describe the unfilled tail of
            // `out`; flags 0 means block until the generator is ready.
            let n =
                unsafe { libc::getrandom(out[have..].as_mut_ptr().cast(), KERNEL_BYTES - have, 0) };
            if n > 0 {
                have += n as usize;
            } else if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        out
    }
}

/// SK-14: the keyboards, read through the kernel's input events. Devices that
/// appear later (a USB keyboard enumerates after the modules load) are found
/// by looking again every half second.
pub struct Keyboards {
    open: Vec<(PathBuf, File)>,
}

impl Keyboards {
    pub fn new() -> Self {
        Self { open: Vec::new() }
    }

    fn rescan(&mut self) {
        let Ok(rd) = fs::read_dir("/dev/input") else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let named = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("event"));
            if !named || self.open.iter().any(|(q, _)| *q == p) {
                continue;
            }
            if let Ok(f) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&p)
            {
                self.open.push((p, f));
                #[cfg(feature = "serial-echo")]
                serial_inputs();
            }
        }
    }

    /// Waits for the next key the program uses.
    pub fn next_key(&mut self) -> Key {
        loop {
            self.rescan();
            let mut fds: Vec<libc::pollfd> = self
                .open
                .iter()
                .map(|(_, f)| libc::pollfd {
                    fd: f.as_raw_fd() as RawFd,
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            // SAFETY: `fds` is a valid array of `fds.len()` pollfd structs.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 500) };
            if n <= 0 {
                continue;
            }
            for (i, pfd) in fds.iter().enumerate() {
                if pfd.revents & libc::POLLIN == 0 {
                    continue;
                }
                let mut raw = [0u8; EVENT_SIZE * 32];
                let Ok(got) = (&self.open[i].1).read(&mut raw) else {
                    continue;
                };
                for ev in raw[..got].as_chunks::<EVENT_SIZE>().0 {
                    if let Some(k) = key_from_event(ev) {
                        return k;
                    }
                }
            }
        }
    }
}

/// SK-13, SK-29: powers the computer off. The words are gone with the memory.
pub fn power_off() -> ! {
    // SAFETY: `reboot` with RB_POWER_OFF takes no pointers.
    unsafe { libc::reboot(libc::RB_POWER_OFF) };
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// SK-40: copies a screen's text to the first serial port. Not in a release.
#[cfg(feature = "serial-echo")]
pub fn serial_echo(screen: &openbmt_stick::screen::Screen) {
    use std::io::Write;
    if let Ok(mut f) = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open("/dev/ttyS0")
    {
        let _ = f.write_all(openbmt_stick::screen::echo_text(screen).as_bytes());
    }
}

/// SK-40: the names of the input devices the kernel has, for the test that a
/// USB keyboard's driver was loaded. Not in a release.
#[cfg(feature = "serial-echo")]
pub fn serial_inputs() {
    use std::io::Write;
    let mut names: Vec<String> = fs::read_dir("/sys/class/input")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| read_trim(&format!("{}/name", e.path().display())))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    if let Ok(mut f) = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open("/dev/ttyS0")
    {
        let _ = writeln!(f, "OBM-ECHO INPUTS {}", names.join(" | "));
    }
}

/// SK-40: one line to the serial port. Not in a release.
#[cfg(feature = "serial-echo")]
pub fn serial_note(line: &str) {
    use std::io::Write;
    if let Ok(mut f) = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open("/dev/ttyS0")
    {
        let _ = writeln!(f, "OBM-ECHO {line}");
    }
}
