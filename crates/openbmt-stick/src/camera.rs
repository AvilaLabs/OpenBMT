//! SK-52: the first camera that captures, read through V4L2 with raw ioctls
//! (no library, as the display code does with DRM). The camera is asked for
//! YUYV at 640 x 480, or the nearest size it offers in YUYV, with memory
//! mapped buffers; the Y bytes of a frame are its grey image. This is part
//! of the program that runs as process 1; each use of unsafe code says why
//! it is sound.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

/// Buffers asked of the driver.
const BUFFERS: u32 = 4;
/// The size asked for first.
const WANT_WIDTH: u32 = 640;
const WANT_HEIGHT: u32 = 480;

const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const V4L2_MEMORY_MMAP: u32 = 1;
const V4L2_FIELD_NONE: u32 = 1;
/// `v4l2_fourcc('Y', 'U', 'Y', 'V')`.
const PIX_YUYV: u32 = 0x5659_5559;
const CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
const CAP_STREAMING: u32 = 0x0400_0000;
const CAP_DEVICE_CAPS: u32 = 0x8000_0000;

const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

/// `_IOC(dir, 'V', nr, size)` of `linux/ioctl.h`.
const fn ioc(dir: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((dir << 30) | ((size as u32) << 16) | ((b'V' as u32) << 8) | nr) as libc::c_ulong
}

#[repr(C)]
struct Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pix {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    private: u32,
    flags: u32,
    ycbcr_enc: u32,
    quantization: u32,
    xfer_func: u32,
}

/// `struct v4l2_format` on x86_64: the type, 4 bytes of padding (the union
/// holds pointers) and the 200 bytes of the union, of which `pix` is the
/// first 48.
#[repr(C)]
struct Format {
    kind: u32,
    pad: u32,
    pix: Pix,
    rest: [u8; 152],
}

#[repr(C)]
struct RequestBuffers {
    count: u32,
    kind: u32,
    memory: u32,
    capabilities: u32,
    flags: u8,
    reserved: [u8; 3],
}

/// `struct v4l2_buffer` on x86_64 (88 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct Buffer {
    index: u32,
    kind: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    timestamp: [i64; 2],
    timecode: [u32; 4],
    sequence: u32,
    memory: u32,
    /// The union `m`: `offset` for memory mapped buffers.
    offset: u64,
    length: u32,
    reserved2: u32,
    request_fd: u32,
}

const _: () = assert!(std::mem::size_of::<Capability>() == 104);
const _: () = assert!(std::mem::size_of::<Format>() == 208);
const _: () = assert!(std::mem::size_of::<RequestBuffers>() == 20);
const _: () = assert!(std::mem::size_of::<Buffer>() == 88);

const VIDIOC_QUERYCAP: libc::c_ulong = ioc(IOC_READ, 0, 104);
const VIDIOC_S_FMT: libc::c_ulong = ioc(IOC_READ | IOC_WRITE, 5, 208);
const VIDIOC_REQBUFS: libc::c_ulong = ioc(IOC_READ | IOC_WRITE, 8, 20);
const VIDIOC_QUERYBUF: libc::c_ulong = ioc(IOC_READ | IOC_WRITE, 9, 88);
const VIDIOC_QBUF: libc::c_ulong = ioc(IOC_READ | IOC_WRITE, 15, 88);
const VIDIOC_DQBUF: libc::c_ulong = ioc(IOC_READ | IOC_WRITE, 17, 88);
const VIDIOC_STREAMON: libc::c_ulong = ioc(IOC_WRITE, 18, 4);

/// One ioctl on `file` with a pointer to `arg`.
fn call<T>(file: &File, request: libc::c_ulong, arg: &mut T) -> io::Result<()> {
    // SAFETY: `arg` is a valid, exclusive reference to a `repr(C)` struct of
    // exactly the size the request number encodes (checked by the const
    // assertions above), and the descriptor is open for the call.
    let r = unsafe { libc::ioctl(file.as_raw_fd(), request as _, arg as *mut T) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// An open camera that is streaming.
pub struct Camera {
    file: File,
    maps: Vec<(*mut libc::c_void, usize)>,
    width: usize,
    height: usize,
    stride: usize,
}

impl Camera {
    /// The first `/dev/video*` that is a capture device and gives YUYV, or
    /// `None` (a camera that has not enumerated yet is found by asking
    /// again).
    pub fn open_first() -> Option<Camera> {
        let mut names: Vec<(u32, String)> = fs::read_dir("/dev")
            .ok()?
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let n = name.strip_prefix("video")?.parse::<u32>().ok()?;
                Some((n, name))
            })
            .collect();
        names.sort();
        names
            .iter()
            .find_map(|(_, name)| Self::open(&format!("/dev/{name}")).ok())
    }

    fn open(path: &str) -> io::Result<Camera> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        // SAFETY: an all-zero capability struct is valid; the kernel fills it.
        let mut cap: Capability = unsafe { std::mem::zeroed() };
        call(&file, VIDIOC_QUERYCAP, &mut cap)?;
        let caps = if cap.capabilities & CAP_DEVICE_CAPS != 0 {
            cap.device_caps
        } else {
            cap.capabilities
        };
        if caps & CAP_VIDEO_CAPTURE == 0 || caps & CAP_STREAMING == 0 {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        let mut fmt = Format {
            kind: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            pad: 0,
            pix: Pix {
                width: WANT_WIDTH,
                height: WANT_HEIGHT,
                pixelformat: PIX_YUYV,
                field: V4L2_FIELD_NONE,
                bytesperline: 0,
                sizeimage: 0,
                colorspace: 0,
                private: 0,
                flags: 0,
                ycbcr_enc: 0,
                quantization: 0,
                xfer_func: 0,
            },
            rest: [0; 152],
        };
        // The driver changes the size to the nearest it offers.
        call(&file, VIDIOC_S_FMT, &mut fmt)?;
        if fmt.pix.pixelformat != PIX_YUYV || fmt.pix.width == 0 || fmt.pix.height == 0 {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        let (width, height) = (fmt.pix.width as usize, fmt.pix.height as usize);
        let stride = if fmt.pix.bytesperline == 0 {
            width * 2
        } else {
            fmt.pix.bytesperline as usize
        };
        let mut req = RequestBuffers {
            count: BUFFERS,
            kind: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            memory: V4L2_MEMORY_MMAP,
            capabilities: 0,
            flags: 0,
            reserved: [0; 3],
        };
        call(&file, VIDIOC_REQBUFS, &mut req)?;
        let mut camera = Camera {
            file,
            maps: Vec::new(),
            width,
            height,
            stride,
        };
        for index in 0..req.count {
            let mut buf = new_buffer(index);
            call(&camera.file, VIDIOC_QUERYBUF, &mut buf)?;
            // SAFETY: maps `length` bytes the driver reported for this
            // buffer at its offset, shared with the driver; the result is
            // checked, and the mapping is released in `Drop`.
            let p = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    buf.length as usize,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    camera.file.as_raw_fd(),
                    buf.offset as libc::off_t,
                )
            };
            if p == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }
            camera.maps.push((p, buf.length as usize));
            call(&camera.file, VIDIOC_QBUF, &mut buf)?;
        }
        let mut kind = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        call(&camera.file, VIDIOC_STREAMON, &mut kind)?;
        Ok(camera)
    }

    /// The frame's width and height in pixels.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// The newest frame that is ready, as grey levels (the Y bytes), or
    /// `None` when there is none yet. Older ready frames are given back to
    /// the driver unread.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        let mut latest: Option<Buffer> = None;
        loop {
            let mut buf = new_buffer(0);
            if call(&self.file, VIDIOC_DQBUF, &mut buf).is_err() {
                break;
            }
            if let Some(mut old) = latest.replace(buf) {
                let _ = call(&self.file, VIDIOC_QBUF, &mut old);
            }
        }
        let mut buf = latest?;
        let &(ptr, len) = self.maps.get(buf.index as usize)?;
        let used = (buf.bytesused as usize).min(len);
        // SAFETY: the mapping is `len` bytes long and lives until `Drop`;
        // `used` is no more than that, and the driver does not write to a
        // buffer that has been dequeued until it is queued again below.
        let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, used) };
        let mut luma = vec![0u8; self.width * self.height];
        for (y, row) in luma.chunks_mut(self.width).enumerate() {
            let line = data.get(y * self.stride..).unwrap_or(&[]);
            for (x, px) in row.iter_mut().enumerate() {
                *px = line.get(2 * x).copied().unwrap_or(0);
            }
        }
        let _ = call(&self.file, VIDIOC_QBUF, &mut buf);
        Some(luma)
    }
}

fn new_buffer(index: u32) -> Buffer {
    Buffer {
        index,
        kind: V4L2_BUF_TYPE_VIDEO_CAPTURE,
        bytesused: 0,
        flags: 0,
        field: 0,
        timestamp: [0; 2],
        timecode: [0; 4],
        sequence: 0,
        memory: V4L2_MEMORY_MMAP,
        offset: 0,
        length: 0,
        reserved2: 0,
        request_fd: 0,
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        for (p, len) in self.maps.drain(..) {
            // SAFETY: each pair is a mapping made in `open` and not yet
            // released.
            unsafe { libc::munmap(p, len) };
        }
    }
}
