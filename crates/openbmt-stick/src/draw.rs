//! Drawing a screen (SK-14): large text from the 10 x 20 font, scaled, and a
//! QR code, into a buffer of `0x00RRGGBB` pixels; then packing the buffer in
//! the pixel format the frame buffer reports, whatever its depth.

use crate::font::{FIRST, GLYPHS, GLYPH_H, GLYPH_W, LAST};
use crate::screen::Screen;

const BACKGROUND: u32 = 0x0010_1820;
const TEXT: u32 = 0x00ff_ffff;
const PAPER: u32 = 0x00ff_ffff;
const INK: u32 = 0x0000_0000;
/// Light modules around a QR code (the quiet zone of the standard).
const QUIET: usize = 4;

/// The text scale for a display `width` pixels across: large, at least 40
/// columns.
pub fn text_scale(width: usize) -> usize {
    let wanted = (width / 500).max(2);
    let fits = width / (GLYPH_W * 40);
    wanted.min(fits).max(1)
}

/// Wraps `text` into lines of at most `cols` characters.
pub fn wrap(text: &str, cols: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let cols = cols.max(1);
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(cols).map(|c| c.iter().collect()).collect()
}

/// The pixels of a screen, `0x00RRGGBB` each, row by row.
struct Canvas {
    buf: Vec<u32>,
    w: usize,
    h: usize,
}

impl Canvas {
    fn fill(&mut self, x: usize, y: usize, rw: usize, rh: usize, px: u32) {
        for yy in y..(y + rh).min(self.h) {
            for xx in x..(x + rw).min(self.w) {
                self.buf[yy * self.w + xx] = px;
            }
        }
    }
}

fn glyph(c: char) -> &'static [u16; GLYPH_H] {
    let b = u8::try_from(c)
        .ok()
        .filter(|b| (FIRST..=LAST).contains(b))
        .unwrap_or(b'?');
    &GLYPHS[usize::from(b - FIRST)]
}

fn draw_char(c: &mut Canvas, x: usize, y: usize, scale: usize, ch: char) {
    for (row, bits) in glyph(ch).iter().enumerate() {
        for col in 0..GLYPH_W {
            if bits >> (GLYPH_W - 1 - col) & 1 == 1 {
                c.fill(x + col * scale, y + row * scale, scale, scale, TEXT);
            }
        }
    }
}

/// Draws the screen into `w * h` pixels.
pub fn render(screen: &Screen, w: usize, h: usize) -> Vec<u32> {
    let mut c = Canvas {
        buf: vec![BACKGROUND; w * h],
        w,
        h,
    };
    let margin = (w / 40).max(8);
    let mut text_x = margin;
    if let Some(qr) = &screen.qr {
        let modules = qr.size() + 2 * QUIET;
        let side = (h.saturating_sub(2 * margin)).min(w * 3 / 5);
        let px = (side / modules).max(1);
        c.fill(margin, margin, modules * px, modules * px, PAPER);
        for y in 0..qr.size() {
            for x in 0..qr.size() {
                if qr.get(x, y) {
                    c.fill(
                        margin + (x + QUIET) * px,
                        margin + (y + QUIET) * px,
                        px,
                        px,
                        INK,
                    );
                }
            }
        }
        text_x = 2 * margin + modules * px;
    }
    // With a code beside it the text is one step smaller.
    let scale = if screen.qr.is_some() {
        (text_scale(w) / 2).max(1)
    } else {
        text_scale(w)
    };
    let cols = w.saturating_sub(text_x + margin) / (GLYPH_W * scale);
    let line_h = (GLYPH_H + 4) * scale;
    let mut y = margin;
    for line in &screen.lines {
        for part in wrap(line, cols) {
            if y + GLYPH_H * scale > h {
                return c.buf;
            }
            for (i, ch) in part.chars().enumerate() {
                draw_char(&mut c, text_x + i * GLYPH_W * scale, y, scale, ch);
            }
            y += line_h;
        }
    }
    c.buf
}

/// One colour channel of the frame buffer: its first bit and its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    pub offset: u32,
    pub length: u32,
}

/// The pixel format the frame buffer reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelFormat {
    pub bits_per_pixel: u32,
    pub red: Channel,
    pub green: Channel,
    pub blue: Channel,
}

fn channel(value: u32, c: Channel) -> u32 {
    if c.length == 0 || c.length > 8 {
        return 0;
    }
    (value >> (8 - c.length)) << c.offset
}

/// One `0x00RRGGBB` pixel in the frame buffer's format.
pub fn pack(px: u32, f: &PixelFormat) -> u32 {
    channel((px >> 16) & 0xff, f.red)
        | channel((px >> 8) & 0xff, f.green)
        | channel(px & 0xff, f.blue)
}

/// Writes a row of pixels into `dst` (little-endian, 16, 24 or 32 bits a
/// pixel). Pixels that do not fit are dropped.
pub fn write_row(dst: &mut [u8], src: &[u32], f: &PixelFormat) {
    let bytes = (f.bits_per_pixel / 8) as usize;
    if !(2..=4).contains(&bytes) {
        return;
    }
    for (px, out) in src.iter().zip(dst.chunks_exact_mut(bytes)) {
        let v = pack(*px, f).to_le_bytes();
        out.copy_from_slice(&v[..bytes]);
    }
}
