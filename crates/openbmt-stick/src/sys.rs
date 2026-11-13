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
use std::time::{Duration, Instant};

use bitcoin::hashes::{sha256, Hash};
use openbmt_stick::app::{Chance, Facts};
use openbmt_stick::draw::{self, Channel, PixelFormat};
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

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

/// `struct fb_var_screeninfo` of `linux/fb.h`.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FbVar {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transp: FbBitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

/// `struct fb_fix_screeninfo` of `linux/fb.h`.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FbFix {
    id: [u8; 16],
    smem_start: libc::c_ulong,
    smem_len: u32,
    kind: u32,
    kind_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: libc::c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;
const KDSETMODE: libc::c_ulong = 0x4b3a;
const KD_GRAPHICS: libc::c_ulong = 1;

/// SK-14: the frame buffer, mapped into memory.
pub struct Screen {
    _file: File,
    map: *mut u8,
    map_len: usize,
    width: usize,
    height: usize,
    line: usize,
    format: PixelFormat,
    // The text console is told to stop drawing; kept open to stay so.
    _tty: Option<File>,
}

impl Screen {
    /// Opens `/dev/fb0`, waiting up to `wait` for it to appear.
    pub fn open(wait: Duration) -> io::Result<Screen> {
        let start = Instant::now();
        let file = loop {
            match OpenOptions::new().read(true).write(true).open("/dev/fb0") {
                Ok(f) => break f,
                Err(e) if start.elapsed() >= wait => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        let mut var = FbVar::default();
        let mut fix = FbFix::default();
        let fd = file.as_raw_fd();
        // SAFETY: both structs are `repr(C)` copies of the kernel's and are
        // valid for writes of their size; the ioctls fill them.
        let (a, b) = unsafe {
            (
                libc::ioctl(fd, FBIOGET_VSCREENINFO as _, &mut var as *mut FbVar),
                libc::ioctl(fd, FBIOGET_FSCREENINFO as _, &mut fix as *mut FbFix),
            )
        };
        if a != 0 || b != 0 {
            return Err(io::Error::last_os_error());
        }
        let line = fix.line_length as usize;
        let map_len = fix.smem_len as usize;
        if line == 0 || map_len == 0 || var.bits_per_pixel % 8 != 0 {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        // SAFETY: maps the frame buffer's memory shared and writable for
        // `smem_len` bytes; the result is checked before use.
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let ch = |b: FbBitfield| Channel {
            offset: b.offset,
            length: b.length,
        };
        // Stop the text console drawing over the screen (best effort).
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .ok();
        if let Some(t) = &tty {
            // SAFETY: a plain ioctl on an open terminal; no pointers.
            let _ = unsafe { libc::ioctl(t.as_raw_fd(), KDSETMODE as _, KD_GRAPHICS) };
        }
        Ok(Screen {
            _file: file,
            map: map.cast(),
            map_len,
            width: var.xres as usize,
            height: var.yres as usize,
            line,
            format: PixelFormat {
                bits_per_pixel: var.bits_per_pixel,
                red: ch(var.red),
                green: ch(var.green),
                blue: ch(var.blue),
            },
            _tty: tty,
        })
    }

    /// Draws a screen of the program.
    pub fn show(&mut self, screen: &openbmt_stick::screen::Screen) {
        let buf = draw::render(screen, self.width, self.height);
        // SAFETY: `map` is a live mapping of `map_len` bytes, owned by this
        // value until drop; no other reference to it exists.
        let mem = unsafe { std::slice::from_raw_parts_mut(self.map, self.map_len) };
        let bytes = (self.format.bits_per_pixel / 8) as usize;
        for (y, row) in buf.chunks_exact(self.width).enumerate() {
            let start = y * self.line;
            if let Some(dst) = mem.get_mut(start..start + self.width * bytes) {
                draw::write_row(dst, row, &self.format);
            }
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        // SAFETY: unmaps the mapping made in `open`, once.
        unsafe { libc::munmap(self.map.cast(), self.map_len) };
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
