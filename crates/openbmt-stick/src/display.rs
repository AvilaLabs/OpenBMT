//! The display (SK-14). The program draws through the kernel's display
//! driver (DRM): one buffer of 32-bit pixels that it makes the screen show,
//! whatever depth the display has; the kernel converts. Where that is not
//! there it writes the frame buffer device (`/dev/fb0`) in the pixel format
//! the device reports, whatever its depth. Each use of unsafe code says why it
//! is sound.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

use openbmt_stick::draw::{self, Channel, PixelFormat};
use openbmt_stick::screen::Screen as Page;

/// The display, by whichever way it opened.
pub enum Screen {
    Drm(Drm),
    Fb(Fb),
}

impl Screen {
    /// Opens the display, waiting up to `wait` for the kernel to make one.
    pub fn open(wait: Duration) -> io::Result<Screen> {
        let start = Instant::now();
        loop {
            match Drm::open() {
                Ok(d) => return Ok(Screen::Drm(d)),
                Err(_) => {
                    if let Ok(f) = Fb::open() {
                        return Ok(Screen::Fb(f));
                    }
                }
            }
            if start.elapsed() >= wait {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Draws a screen of the program.
    pub fn show(&mut self, page: &Page) {
        match self {
            Screen::Drm(d) => d.show(page),
            Screen::Fb(f) => f.show(page),
        }
    }
}

// ---- DRM ---------------------------------------------------------------------

/// `struct drm_mode_modeinfo`.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ModeInfo {
    clock: u32,
    hdisplay: u16,
    hsync_start: u16,
    hsync_end: u16,
    htotal: u16,
    hskew: u16,
    vdisplay: u16,
    vsync_start: u16,
    vsync_end: u16,
    vtotal: u16,
    vscan: u16,
    vrefresh: u32,
    flags: u32,
    kind: u32,
    name: [u8; 32],
}

/// `struct drm_mode_card_res`.
#[repr(C)]
#[derive(Default)]
struct CardRes {
    fb_id_ptr: u64,
    crtc_id_ptr: u64,
    connector_id_ptr: u64,
    encoder_id_ptr: u64,
    count_fbs: u32,
    count_crtcs: u32,
    count_connectors: u32,
    count_encoders: u32,
    min_width: u32,
    max_width: u32,
    min_height: u32,
    max_height: u32,
}

/// `struct drm_mode_get_connector`.
#[repr(C)]
#[derive(Default)]
struct GetConnector {
    encoders_ptr: u64,
    modes_ptr: u64,
    props_ptr: u64,
    prop_values_ptr: u64,
    count_modes: u32,
    count_props: u32,
    count_encoders: u32,
    encoder_id: u32,
    connector_id: u32,
    connector_type: u32,
    connector_type_id: u32,
    connection: u32,
    mm_width: u32,
    mm_height: u32,
    subpixel: u32,
    pad: u32,
}

/// `struct drm_mode_create_dumb`.
#[repr(C)]
#[derive(Default)]
struct CreateDumb {
    height: u32,
    width: u32,
    bpp: u32,
    flags: u32,
    handle: u32,
    pitch: u32,
    size: u64,
}

/// `struct drm_mode_fb_cmd`.
#[repr(C)]
#[derive(Default)]
struct FbCmd {
    fb_id: u32,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u32,
    depth: u32,
    handle: u32,
}

/// `struct drm_mode_map_dumb`.
#[repr(C)]
#[derive(Default)]
struct MapDumb {
    handle: u32,
    pad: u32,
    offset: u64,
}

/// `struct drm_mode_crtc`.
#[repr(C)]
#[derive(Default)]
struct Crtc {
    set_connectors_ptr: u64,
    count_connectors: u32,
    crtc_id: u32,
    fb_id: u32,
    x: u32,
    y: u32,
    gamma_size: u32,
    mode_valid: u32,
    mode: ModeInfo,
}

/// `struct drm_mode_fb_dirty_cmd`.
#[repr(C)]
#[derive(Default)]
struct FbDirty {
    fb_id: u32,
    flags: u32,
    color: u32,
    num_clips: u32,
    clips_ptr: u64,
}

const GETRESOURCES: u32 = 0xa0;
const SETCRTC: u32 = 0xa2;
const GETCONNECTOR: u32 = 0xa7;
const ADDFB: u32 = 0xae;
const DIRTYFB: u32 = 0xb1;
const CREATE_DUMB: u32 = 0xb2;
const MAP_DUMB: u32 = 0xb3;

/// One DRM ioctl: `DRM_IOWR('d', nr, T)`.
fn drm_ioctl<T>(fd: RawFd, nr: u32, arg: &mut T) -> io::Result<()> {
    let req = (3u32 << 30) | ((std::mem::size_of::<T>() as u32) << 16) | (0x64 << 8) | nr;
    loop {
        // SAFETY: `arg` is a `repr(C)` copy of the kernel's struct for this
        // request, valid for reads and writes of its size; the pointers
        // inside it point at buffers that outlive the call.
        let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
        if r == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// The display through DRM: a dumb buffer shown on the first connected output.
pub struct Drm {
    file: File,
    fb_id: u32,
    width: usize,
    height: usize,
    pitch: usize,
    map: *mut u8,
    map_len: usize,
}

impl Drm {
    fn open() -> io::Result<Drm> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/dri/card0")?;
        let fd = file.as_raw_fd();
        let mut res = CardRes::default();
        drm_ioctl(fd, GETRESOURCES, &mut res)?;
        let mut crtcs = vec![0u32; res.count_crtcs as usize];
        let mut conns = vec![0u32; res.count_connectors as usize];
        let mut res = CardRes {
            crtc_id_ptr: crtcs.as_mut_ptr() as u64,
            connector_id_ptr: conns.as_mut_ptr() as u64,
            count_crtcs: crtcs.len() as u32,
            count_connectors: conns.len() as u32,
            ..CardRes::default()
        };
        drm_ioctl(fd, GETRESOURCES, &mut res)?;
        let crtc_id = *crtcs
            .first()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        // The first connector with a mode: its first mode is the one the
        // firmware set.
        let mut chosen = None;
        for conn in &conns {
            let mut probe = GetConnector {
                connector_id: *conn,
                ..GetConnector::default()
            };
            drm_ioctl(fd, GETCONNECTOR, &mut probe)?;
            if probe.count_modes == 0 {
                continue;
            }
            let mut modes = vec![ModeInfo::default(); probe.count_modes as usize];
            let mut get = GetConnector {
                connector_id: *conn,
                modes_ptr: modes.as_mut_ptr() as u64,
                count_modes: modes.len() as u32,
                ..GetConnector::default()
            };
            drm_ioctl(fd, GETCONNECTOR, &mut get)?;
            if get.connection != 2 {
                chosen = Some((*conn, modes[0]));
                break;
            }
        }
        let (conn, mode) = chosen.ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let (width, height) = (usize::from(mode.hdisplay), usize::from(mode.vdisplay));
        let mut dumb = CreateDumb {
            width: width as u32,
            height: height as u32,
            bpp: 32,
            ..CreateDumb::default()
        };
        drm_ioctl(fd, CREATE_DUMB, &mut dumb)?;
        let mut fb = FbCmd {
            width: width as u32,
            height: height as u32,
            pitch: dumb.pitch,
            bpp: 32,
            depth: 24,
            handle: dumb.handle,
            ..FbCmd::default()
        };
        drm_ioctl(fd, ADDFB, &mut fb)?;
        let mut map = MapDumb {
            handle: dumb.handle,
            ..MapDumb::default()
        };
        drm_ioctl(fd, MAP_DUMB, &mut map)?;
        let map_len = dumb.size as usize;
        // SAFETY: maps the dumb buffer, shared and writable, at the offset the
        // kernel gave for `map_len` bytes; the result is checked before use.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                map.offset as libc::off_t,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let mut connectors = [conn];
        let mut crtc = Crtc {
            set_connectors_ptr: connectors.as_mut_ptr() as u64,
            count_connectors: 1,
            crtc_id,
            fb_id: fb.fb_id,
            mode_valid: 1,
            mode,
            ..Crtc::default()
        };
        if let Err(e) = drm_ioctl(fd, SETCRTC, &mut crtc) {
            // SAFETY: unmaps the mapping made above, once.
            unsafe { libc::munmap(ptr, map_len) };
            return Err(e);
        }
        #[cfg(feature = "serial-echo")]
        crate::sys::serial_note(&format!(
            "DRM {width}x{height} pitch {} size {map_len}",
            dumb.pitch
        ));
        Ok(Drm {
            file,
            fb_id: fb.fb_id,
            width,
            height,
            pitch: dumb.pitch as usize,
            map: ptr.cast(),
            map_len,
        })
    }

    fn show(&mut self, page: &Page) {
        let buf = draw::render(page, self.width, self.height);
        // SAFETY: `map` is a live mapping of `map_len` bytes, owned by this
        // value until drop; no other reference to it exists.
        let mem = unsafe { std::slice::from_raw_parts_mut(self.map, self.map_len) };
        for (y, row) in buf.chunks_exact(self.width).enumerate() {
            let start = y * self.pitch;
            if let Some(dst) = mem.get_mut(start..start + self.width * 4) {
                for (px, out) in row.iter().zip(dst.as_chunks_mut::<4>().0.iter_mut()) {
                    out.copy_from_slice(&px.to_le_bytes());
                }
            }
        }
        // Tell the kernel the whole buffer changed, so it copies it to the
        // display now.
        let mut dirty = FbDirty {
            fb_id: self.fb_id,
            ..FbDirty::default()
        };
        let _ = drm_ioctl(self.file.as_raw_fd(), DIRTYFB, &mut dirty);
    }
}

impl Drop for Drm {
    fn drop(&mut self) {
        // SAFETY: unmaps the mapping made in `open`, once.
        unsafe { libc::munmap(self.map.cast(), self.map_len) };
    }
}

// ---- the frame buffer device ---------------------------------------------------

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
const FBIOPUT_VSCREENINFO: libc::c_ulong = 0x4601;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;
/// `FB_ACTIVATE_FORCE`: set the mode even if it is the one already set.
const FB_ACTIVATE_FORCE: u32 = 128;

/// The display through `/dev/fb0`.
pub struct Fb {
    file: File,
    width: usize,
    height: usize,
    line: usize,
    format: PixelFormat,
}

impl Fb {
    fn open() -> io::Result<Fb> {
        let file = OpenOptions::new().read(true).write(true).open("/dev/fb0")?;
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
        // Ask the kernel to set the display up for this buffer now.
        let mut set = var;
        set.activate = FB_ACTIVATE_FORCE;
        // SAFETY: `set` is a `repr(C)` copy of the kernel's struct, valid for
        // reads of its size.
        let _ = unsafe { libc::ioctl(fd, FBIOPUT_VSCREENINFO as _, &mut set as *mut FbVar) };
        let line = fix.line_length as usize;
        if line == 0
            || fix.smem_len as usize <= line * var.yres as usize
            || var.bits_per_pixel % 8 != 0
        {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        let ch = |b: FbBitfield| Channel {
            offset: b.offset,
            length: b.length,
        };
        Ok(Fb {
            file,
            width: var.xres as usize,
            height: var.yres as usize,
            line,
            format: PixelFormat {
                bits_per_pixel: var.bits_per_pixel,
                red: ch(var.red),
                green: ch(var.green),
                blue: ch(var.blue),
            },
        })
    }

    fn show(&mut self, page: &Page) {
        let buf = draw::render(page, self.width, self.height);
        let bytes = (self.format.bits_per_pixel / 8) as usize;
        let mut frame = vec![0u8; self.line * self.height];
        for (y, row) in buf.chunks_exact(self.width).enumerate() {
            let start = y * self.line;
            if let Some(dst) = frame.get_mut(start..start + self.width * bytes) {
                draw::write_row(dst, row, &self.format);
            }
        }
        let _ = self.file.write_all_at(&frame, 0);
    }
}
