//! Japanese-capable text rendering for the Switch build.
//!
//! Pipeline:
//!
//!   pl:u shared font   (plInitialize / plGetSharedFontByType, PlSharedFontType_Standard)
//!     -> stb_truetype  (hidden behind switch/tools/font_shim.c, one C file)
//!       -> 8-bit coverage glyph cache (this file)
//!         -> libnx linear framebuffer (nwindow + framebuffer*, PIXEL_FORMAT_RGBA_8888)
//!
//! The public surface is the one the rest of the crate was written against:
//! Canvas::new / width / height / clear / text / rect / present.  Everything
//! else here is an implementation detail, plus two read-only helpers
//! (line_height, text_width), an on-device font probe and an on-device
//! Canvas smoke test used by examples/text_probe.rs.
//!
//! Threading / lifetime: a Canvas owns the default NWindow, its framebuffer
//! and the pl:u session, so exactly one may exist at a time and it must be
//! used from the thread that created it.  It must be dropped before plExit is
//! called by anything else; Drop does framebufferClose + plExit for you.
//!
//! Colours are 0xAABBGGRR.  On a little-endian machine those four bytes are
//! R,G,B,A in memory, which is exactly libnx RGBA8(r,g,b,a)
//! (display/framebuffer.h:12) for PIXEL_FORMAT_RGBA_8888, so no conversion is
//! needed anywhere in this file.  The alpha byte is an opacity multiplier for
//! text() and is written verbatim by clear()/rect().
//!
//! Coordinates: text(x, y, ...) puts the top of the line box at y (the pen
//! baseline is y + ascent), one line only.  A newline stops the run.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;

use crate::sys;
use crate::sys::Result as NxResult;

// ---------------------------------------------------------------------------
// libnx FFI
//
// Transcribed from $(DEVKITPRO)/libnx/include (libnx 4.12.0).  Line numbers
// refer to the headers as installed here and are repeated in
// docs/switch_text.md.
// ---------------------------------------------------------------------------

extern "C" {
    // switch/services/pl.h:36, 39, 48
    fn plInitialize(service_type: u32) -> NxResult;
    fn plExit();
    fn plGetSharedFontByType(font: *mut PlFontData, font_type: u32) -> NxResult;

    // switch/display/native_window.h:56, 90
    fn nwindowGetDefault() -> *mut c_void;
    fn nwindowGetDimensions(nw: *mut c_void, out_width: *mut u32, out_height: *mut u32) -> NxResult;

    // switch/display/framebuffer.h:75, 78, 81, 94, 104
    fn framebufferCreate(
        fb: *mut Framebuffer,
        win: *mut c_void,
        width: u32,
        height: u32,
        format: u32,
        num_fbs: u32,
    ) -> NxResult;
    fn framebufferMakeLinear(fb: *mut Framebuffer) -> NxResult;
    fn framebufferClose(fb: *mut Framebuffer);
    fn framebufferBegin(fb: *mut Framebuffer, out_stride: *mut u32) -> *mut c_void;
    fn framebufferEnd(fb: *mut Framebuffer);
}

// switch/tools/font_shim.c - the only C code, so stb_truetype's structs never
// have to be duplicated in Rust.  Return codes of mhnx_font_glyph:
//   -2 not in font, -1 bad handle, 0 rasterised / blank glyph,
//   >0 bytes needed (metrics-only query: out == NULL or out_cap < w*h).
extern "C" {
    fn mhnx_font_create(data: *const u8, len: i32, pixel_height: f32) -> *mut c_void;
    fn mhnx_font_ascent(font: *mut c_void) -> i32;
    fn mhnx_font_descent(font: *mut c_void) -> i32;
    fn mhnx_font_line_gap(font: *mut c_void) -> i32;
    fn mhnx_font_cff(font: *mut c_void) -> i32;
    fn mhnx_font_glyph(
        font: *mut c_void,
        codepoint: i32,
        out: *mut u8,
        out_cap: i32,
        w: *mut i32,
        h: *mut i32,
        xoff: *mut i32,
        yoff: *mut i32,
        advance: *mut i32,
    ) -> i32;
    fn mhnx_font_destroy(font: *mut c_void);
}

// ---------------------------------------------------------------------------
// libnx structures and constants
// ---------------------------------------------------------------------------

/// switch/services/pl.h:28-33
///   typedef struct { u32 type; u32 offset; u32 size; void* address; } PlFontData;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlFontData {
    pub font_type: u32,
    pub offset: u32,
    pub size: u32,
    pub address: *mut c_void,
}

impl PlFontData {
    const ZERO: Self = Self {
        font_type: 0,
        offset: 0,
        size: 0,
        address: core::ptr::null_mut(),
    };
}

/// switch/nvidia/map.h:4-12 (NvKind is a C enum, i.e. 32 bit)
#[repr(C)]
#[derive(Clone, Copy)]
struct NvMap {
    handle: u32,
    id: u32,
    size: u32,
    cpu_addr: *mut c_void,
    kind: u32,
    has_init: bool,
    is_cpu_cacheable: bool,
}

/// switch/display/framebuffer.h:45-56
#[repr(C)]
#[derive(Clone, Copy)]
struct Framebuffer {
    win: *mut c_void,
    map: NvMap,
    buf: *mut c_void,
    buf_linear: *mut c_void,
    stride: u32,
    width_aligned: u32,
    height_aligned: u32,
    num_fbs: u32,
    fb_size: u32,
    has_init: bool,
}

impl Framebuffer {
    const ZERO: Self = Self {
        win: core::ptr::null_mut(),
        map: NvMap {
            handle: 0,
            id: 0,
            size: 0,
            cpu_addr: core::ptr::null_mut(),
            kind: 0,
            has_init: false,
            is_cpu_cacheable: false,
        },
        buf: core::ptr::null_mut(),
        buf_linear: core::ptr::null_mut(),
        stride: 0,
        width_aligned: 0,
        height_aligned: 0,
        num_fbs: 0,
        fb_size: 0,
        has_init: false,
    };
}

// Layout checks.  These numbers were verified against the installed headers
// with a devkitA64 C translation unit full of _Static_asserts (see
// docs/switch_text.md); if libnx ever changes, this file fails to compile
// instead of corrupting the stack.
const _: () = {
    use core::mem::offset_of;
    use core::mem::size_of;

    assert!(size_of::<PlFontData>() == 24);
    assert!(offset_of!(PlFontData, font_type) == 0);
    assert!(offset_of!(PlFontData, offset) == 4);
    assert!(offset_of!(PlFontData, size) == 8);
    assert!(offset_of!(PlFontData, address) == 16);

    assert!(size_of::<NvMap>() == 32);
    assert!(offset_of!(NvMap, cpu_addr) == 16);
    assert!(offset_of!(NvMap, kind) == 24);

    assert!(size_of::<Framebuffer>() == 80);
    assert!(offset_of!(Framebuffer, win) == 0);
    assert!(offset_of!(Framebuffer, map) == 8);
    assert!(offset_of!(Framebuffer, buf) == 40);
    assert!(offset_of!(Framebuffer, buf_linear) == 48);
    assert!(offset_of!(Framebuffer, stride) == 56);
    assert!(offset_of!(Framebuffer, width_aligned) == 60);
    assert!(offset_of!(Framebuffer, height_aligned) == 64);
    assert!(offset_of!(Framebuffer, num_fbs) == 68);
    assert!(offset_of!(Framebuffer, fb_size) == 72);
    assert!(offset_of!(Framebuffer, has_init) == 76);
};

/// PlServiceType_User (pl.h:12).  System (1) lost shared-font access in
/// firmware 16.0.0, so pl:u is the only safe choice.
const PL_SERVICE_TYPE_USER: u32 = 0;
/// PlSharedFontType_Standard (pl.h:18): Japan / US / Europe, contains kana and
/// the JIS kanji set.
const PL_SHARED_FONT_TYPE_STANDARD: u32 = 0;
/// PIXEL_FORMAT_RGBA_8888 (switch/display/types.h:11)
const PIXEL_FORMAT_RGBA_8888: u32 = 1;

const FB_WIDTH: u32 = 1280;
const FB_HEIGHT: u32 = 720;
const FB_COUNT: u32 = 2;

/// Glyph rasterisation height.  24 px gives ~24 readable menu lines on 720p
/// and keeps the shared font below a millisecond per new glyph on the A57.
const FONT_PIXEL_HEIGHT: f32 = 24.0;

/// The glyph cache is dropped wholesale above this size: 24x24 kanji are
/// ~576 bytes each, so 2 MiB holds >3000 distinct characters.
const MAX_CACHE_BYTES: usize = 2 * 1024 * 1024;

/// Advance used when neither the character nor '?' exists in the font.
const FALLBACK_ADVANCE: i32 = 12;

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

/// A 32 bit RGBA colour in the framebuffer's native order (0xAABBGGRR).
pub type Color = u32;

/// Opaque black, convenient as a clear() argument.
pub const BLACK: Color = 0xFF00_0000;
/// Opaque white.
pub const WHITE: Color = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// glyph cache
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct GlyphMetrics {
    w: i32,
    h: i32,
    xoff: i32,
    yoff: i32,
    advance: i32,
}

struct CachedGlyph {
    w: i32,
    h: i32,
    xoff: i32,
    yoff: i32,
    advance: i32,
    /// w*h 8-bit coverage bytes, row stride w.  Empty for blank glyphs.
    pixels: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

/// A software framebuffer plus the shared-font rasteriser that draws into it.
pub struct Canvas {
    fb: Framebuffer,
    /// Shadow/back buffer returned by framebufferBegin, null between frames.
    back: *mut u8,
    stride: u32,
    width: i32,
    height: i32,
    font: *mut c_void,
    ascent: i32,
    descent: i32,
    line_gap: i32,
    /// Keyed by char, because the pixel height is a build-time constant.
    /// no_std + alloc has no HashMap without pulling in hashbrown, and a
    /// BTreeMap is more than fast enough for a few hundred glyphs.
    cache: BTreeMap<u32, CachedGlyph>,
    cache_bytes: usize,
}

impl Canvas {
    /// Create the canvas: pl:u session + standard shared font + a linear,
    /// double buffered framebuffer on the default window.
    ///
    /// Returns None when any of those steps fail so the caller can fall back
    /// to libnx' ASCII console.  Note that the console owns the default window
    /// too: call consoleExit() before Canvas::new(), and do not call
    /// consoleInit() again while the Canvas is alive.
    pub fn new() -> Option<Self> {
        unsafe {
            // 1. Shared font.
            if !sys::r_succeeded(plInitialize(PL_SERVICE_TYPE_USER)) {
                return None;
            }
            let mut font_data = PlFontData::ZERO;
            let rc = plGetSharedFontByType(&mut font_data, PL_SHARED_FONT_TYPE_STANDARD);
            if !sys::r_succeeded(rc) || font_data.address.is_null() || font_data.size < 1024 {
                plExit();
                return None;
            }

            // 2. stb_truetype handle.  The data is not copied: pl keeps the
            //    shared memory mapped until plExit().
            let font = mhnx_font_create(
                font_data.address as *const u8,
                font_data.size as i32,
                FONT_PIXEL_HEIGHT,
            );
            if font.is_null() {
                plExit();
                return None;
            }
            let ascent = mhnx_font_ascent(font);
            let descent = mhnx_font_descent(font);
            let line_gap = mhnx_font_line_gap(font);
            if ascent <= 0 {
                mhnx_font_destroy(font);
                plExit();
                return None;
            }

            // 3. Framebuffer.
            let win = nwindowGetDefault();
            if win.is_null() {
                mhnx_font_destroy(font);
                plExit();
                return None;
            }
            let (mut width, mut height) = (FB_WIDTH, FB_HEIGHT);
            if !sys::r_succeeded(nwindowGetDimensions(win, &mut width, &mut height))
                || width == 0
                || height == 0
            {
                width = FB_WIDTH;
                height = FB_HEIGHT;
            }
            let mut fb = Framebuffer::ZERO;
            let rc = framebufferCreate(
                &mut fb,
                win,
                width,
                height,
                PIXEL_FORMAT_RGBA_8888,
                FB_COUNT,
            );
            if !sys::r_succeeded(rc) {
                mhnx_font_destroy(font);
                plExit();
                return None;
            }
            let rc = framebufferMakeLinear(&mut fb);
            if !sys::r_succeeded(rc) {
                framebufferClose(&mut fb);
                mhnx_font_destroy(font);
                plExit();
                return None;
            }

            Some(Canvas {
                fb,
                back: core::ptr::null_mut(),
                stride: 0,
                width: width as i32,
                height: height as i32,
                font,
                ascent,
                descent,
                line_gap,
                cache: BTreeMap::new(),
                cache_bytes: 0,
            })
        }
    }

    /// Screen width in pixels.
    pub fn width(&self) -> i32 {
        self.width
    }

    /// Screen height in pixels.
    pub fn height(&self) -> i32 {
        self.height
    }

    /// Distance between the baselines of two consecutive text() lines.
    pub fn line_height(&self) -> i32 {
        let h = self.ascent - self.descent + self.line_gap;
        if h > 0 {
            h
        } else {
            self.ascent + 4
        }
    }

    /// Pixel width of s when drawn with text().  Not part of the original
    /// stub surface; populates the same glyph cache text() uses.
    pub fn text_width(&mut self, s: &str) -> i32 {
        let mut total = 0i32;
        for raw in s.chars() {
            if raw == '\n' || raw == '\r' {
                break;
            }
            let mut ch = sanitize(raw);
            let mut m = self.ensure_glyph(ch);
            if m.is_none() && ch != '?' {
                ch = '?';
                m = self.ensure_glyph(ch);
            }
            total = total.saturating_add(m.map(|g| g.advance).unwrap_or(FALLBACK_ADVANCE));
        }
        total
    }

    /// Fill the whole screen.  The colour (including alpha) is stored as is.
    pub fn clear(&mut self, color: Color) {
        self.begin_frame();
        if self.back.is_null() {
            return;
        }
        let stride = self.stride as usize;
        let width = self.width.max(0) as usize;
        for y in 0..self.height.max(0) as usize {
            let row = unsafe { self.back.add(y * stride) } as *mut u32;
            for x in 0..width {
                unsafe { core::ptr::write_unaligned(row.add(x), color) };
            }
        }
    }

    /// Draw one line of UTF-8 text with the top of the line box at (x, y).
    ///
    /// Characters outside the BMP (emoji, rare CJK ext. B+) are drawn as '?'.
    /// Characters missing from the shared font also fall back to '?'.  Only
    /// control characters below U+0020 are dropped; '\n' ends the line.
    pub fn text(&mut self, x: i32, y: i32, s: &str, color: Color) {
        self.begin_frame();
        if self.back.is_null() {
            return;
        }
        let baseline = y.saturating_add(self.ascent);
        let mut pen = x;
        for raw in s.chars() {
            if raw == '\n' || raw == '\r' {
                break;
            }
            let mut ch = sanitize(raw);
            let mut metrics = self.ensure_glyph(ch);
            if metrics.is_none() && ch != '?' {
                ch = '?';
                metrics = self.ensure_glyph(ch);
            }
            let Some(m) = metrics else {
                pen = pen.saturating_add(FALLBACK_ADVANCE);
                continue;
            };

            if m.w > 0 && m.h > 0 {
                let back = self.back;
                let stride = self.stride;
                let screen_w = self.width;
                let screen_h = self.height;
                let px = pen.saturating_add(m.xoff);
                let py = baseline.saturating_add(m.yoff);
                if let Some(g) = self.cache.get(&(ch as u32)) {
                    unsafe {
                        blend_glyph(back, stride, screen_w, screen_h, px, py, g, color);
                    }
                }
            }
            pen = pen.saturating_add(m.advance);
        }
    }

    /// Draw a filled rectangle.  The colour (including alpha) is stored as is
    /// and clipped to the screen.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        self.begin_frame();
        if self.back.is_null() || w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = x.saturating_add(w).min(self.width);
        let y1 = y.saturating_add(h).min(self.height);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = self.stride as usize;
        for py in y0..y1 {
            let row = unsafe { self.back.add(py as usize * stride) } as *mut u32;
            for px in x0..x1 {
                unsafe { core::ptr::write_unaligned(row.add(px as usize), color) };
            }
        }
    }

    /// Present the frame (framebufferEnd).  After this the next drawing call
    /// starts a new frame; the back buffer is not preserved between frames, so
    /// a full redraw (clear + content) is expected each time.
    pub fn present(&mut self) {
        if self.back.is_null() {
            return;
        }
        unsafe { framebufferEnd(&mut self.fb) };
        self.back = core::ptr::null_mut();
    }

    /// Rasterise (or fetch from the cache) one glyph.  None means "not in the
    /// font"; the caller substitutes '?'.
    fn ensure_glyph(&mut self, ch: char) -> Option<GlyphMetrics> {
        let key = ch as u32;
        if let Some(g) = self.cache.get(&key) {
            return Some(GlyphMetrics {
                w: g.w,
                h: g.h,
                xoff: g.xoff,
                yoff: g.yoff,
                advance: g.advance,
            });
        }
        if self.cache_bytes >= MAX_CACHE_BYTES {
            self.cache.clear();
            self.cache_bytes = 0;
        }

        let (mut w, mut h, mut xoff, mut yoff, mut advance) = (0i32, 0i32, 0i32, 0i32, 0i32);
        let need = unsafe {
            mhnx_font_glyph(
                self.font,
                key as i32,
                core::ptr::null_mut(),
                0,
                &mut w,
                &mut h,
                &mut xoff,
                &mut yoff,
                &mut advance,
            )
        };
        if need < 0 {
            return None;
        }

        let mut pixels: Vec<u8> = Vec::new();
        if need > 0 && w > 0 && h > 0 {
            let mut buf = vec![0u8; need as usize];
            let rc = unsafe {
                mhnx_font_glyph(
                    self.font,
                    key as i32,
                    buf.as_mut_ptr(),
                    need,
                    &mut w,
                    &mut h,
                    &mut xoff,
                    &mut yoff,
                    &mut advance,
                )
            };
            if rc == 0 {
                self.cache_bytes += buf.len();
                pixels = buf;
            }
        }
        let metrics = GlyphMetrics { w, h, xoff, yoff, advance };
        self.cache.insert(key, CachedGlyph { w, h, xoff, yoff, advance, pixels });
        Some(metrics)
    }

    /// Lazy framebufferBegin: a frame starts with the first drawing call and
    /// ends at present().
    fn begin_frame(&mut self) {
        if !self.back.is_null() || self.width <= 0 || self.height <= 0 {
            return;
        }
        let mut stride = 0u32;
        let ptr = unsafe { framebufferBegin(&mut self.fb, &mut stride) };
        if ptr.is_null() || stride == 0 {
            return;
        }
        self.back = ptr as *mut u8;
        self.stride = stride;
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            if !self.back.is_null() {
                framebufferEnd(&mut self.fb);
                self.back = core::ptr::null_mut();
            }
            framebufferClose(&mut self.fb);
            if !self.font.is_null() {
                mhnx_font_destroy(self.font);
                self.font = core::ptr::null_mut();
            }
            // Must come last: the glyph cache and stb_truetype still point
            // into the shared memory this unmaps.
            plExit();
        }
    }
}

// ---------------------------------------------------------------------------
// drawing primitives
// ---------------------------------------------------------------------------

/// Strip characters the shared font has no use for.  Outside the BMP there is
/// no glyph (and stb_truetype only looks at the BMP anyway), so those become
/// '?'.  Other control characters become a space; '\n'/'\r' are handled by the
/// caller.
fn sanitize(ch: char) -> char {
    let c = ch as u32;
    if c > 0xFFFF {
        '?'
    } else if c < 0x20 {
        ' '
    } else {
        ch
    }
}

#[inline]
fn blend_pixel(dst: Color, color: Color, coverage: u32) -> Color {
    let inv = 255 - coverage;
    let mix = |d: u32, s: u32| (d * inv + s * coverage) / 255;
    let r = mix(dst & 0xFF, color & 0xFF);
    let g = mix((dst >> 8) & 0xFF, (color >> 8) & 0xFF);
    let b = mix((dst >> 16) & 0xFF, (color >> 16) & 0xFF);
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

/// Blend one cached glyph onto the framebuffer with clipping.
unsafe fn blend_glyph(
    back: *mut u8,
    stride: u32,
    screen_w: i32,
    screen_h: i32,
    x0: i32,
    y0: i32,
    g: &CachedGlyph,
    color: Color,
) {
    let expected = (g.w as usize) * (g.h as usize);
    if back.is_null() || expected == 0 || g.pixels.len() < expected {
        return;
    }
    let alpha = (color >> 24) & 0xFF;
    if alpha == 0 {
        return;
    }
    let x_from = x0.max(0);
    let y_from = y0.max(0);
    let x_to = x0.saturating_add(g.w).min(screen_w);
    let y_to = y0.saturating_add(g.h).min(screen_h);
    if x_from >= x_to || y_from >= y_to {
        return;
    }
    for py in y_from..y_to {
        let row = (py as i64 - y0 as i64) as usize;
        let src = g.pixels.as_ptr().add(row * g.w as usize);
        for px in x_from..x_to {
            let cov = *src.add((px as i64 - x0 as i64) as usize) as u32;
            if cov == 0 {
                continue;
            }
            let cov = cov * alpha / 255;
            if cov == 0 {
                continue;
            }
            let dst = back.add(py as usize * stride as usize + px as usize * 4) as *mut u32;
            let old = core::ptr::read_unaligned(dst);
            core::ptr::write_unaligned(dst, blend_pixel(old, color, cov));
        }
    }
}

// ---------------------------------------------------------------------------
// on-device self test / probe
// ---------------------------------------------------------------------------

/// Result of probe_shared_font(): proves the font -> glyph -> bitmap chain
/// without touching the framebuffer.
pub struct FontProbe {
    pub font_address: usize,
    pub font_size: u32,
    pub ascent: i32,
    pub descent: i32,
    /// True when the shared font uses CFF outlines rather than TrueType glyf.
    pub cff: bool,
    pub glyphs: u32,
    pub inked: u32,
    pub blank: u32,
    pub missing: u32,
    pub nonzero_pixels: u32,
    pub first_inked: Option<char>,
}

/// Load PlSharedFontType_Standard through pl:u and rasterise every character
/// of sample.
///
/// None means plInitialize/plGetSharedFontByType/stb_truetype failed, i.e. the
/// Japanese path is unusable and the caller should keep the ASCII console.
/// Some(probe) with inked > 0 means the whole chain works.  This opens its own
/// pl:u session, so call it before Canvas::new() (or after dropping the
/// Canvas) to avoid two framebuffers fighting over the default window - it
/// never touches the framebuffer itself.
pub fn probe_shared_font(sample: &str) -> Option<FontProbe> {
    unsafe {
        if !sys::r_succeeded(plInitialize(PL_SERVICE_TYPE_USER)) {
            return None;
        }
        let mut font_data = PlFontData::ZERO;
        let rc = plGetSharedFontByType(&mut font_data, PL_SHARED_FONT_TYPE_STANDARD);
        if !sys::r_succeeded(rc) || font_data.address.is_null() || font_data.size == 0 {
            plExit();
            return None;
        }
        let font = mhnx_font_create(
            font_data.address as *const u8,
            font_data.size as i32,
            FONT_PIXEL_HEIGHT,
        );
        if font.is_null() {
            plExit();
            return None;
        }

        let mut probe = FontProbe {
            font_address: font_data.address as usize,
            font_size: font_data.size,
            ascent: mhnx_font_ascent(font),
            descent: mhnx_font_descent(font),
            cff: mhnx_font_cff(font) != 0,
            glyphs: 0,
            inked: 0,
            blank: 0,
            missing: 0,
            nonzero_pixels: 0,
            first_inked: None,
        };

        let mut buf: Vec<u8> = Vec::new();
        for raw in sample.chars() {
            let ch = sanitize(raw);
            let (mut w, mut h, mut xoff, mut yoff, mut advance) = (0i32, 0i32, 0i32, 0i32, 0i32);
            let need = mhnx_font_glyph(
                font,
                ch as i32,
                core::ptr::null_mut(),
                0,
                &mut w,
                &mut h,
                &mut xoff,
                &mut yoff,
                &mut advance,
            );
            probe.glyphs += 1;
            if need < 0 {
                probe.missing += 1;
                continue;
            }
            if need == 0 || w <= 0 || h <= 0 {
                probe.blank += 1;
                continue;
            }
            buf.clear();
            buf.resize(need as usize, 0);
            let rc = mhnx_font_glyph(
                font,
                ch as i32,
                buf.as_mut_ptr(),
                need,
                &mut w,
                &mut h,
                &mut xoff,
                &mut yoff,
                &mut advance,
            );
            if rc != 0 {
                probe.missing += 1;
                continue;
            }
            let nonzero = buf.iter().filter(|&&b| b != 0).count() as u32;
            if nonzero > 0 {
                probe.inked += 1;
                probe.nonzero_pixels += nonzero;
                if probe.first_inked.is_none() {
                    probe.first_inked = Some(ch);
                }
            } else {
                probe.blank += 1;
            }
        }

        mhnx_font_destroy(font);
        plExit();
        Some(probe)
    }
}

/// Draw a fixed Japanese/ASCII sample plus a colour bar into a fresh Canvas,
/// count the pixels that differ from the black background, present it, and
/// hand the Canvas back to the caller.
///
/// The Canvas is still presenting the test frame when this returns, so the
/// caller can hold it on screen (and then drop it) before re-initialising the
/// console.  None means Canvas::new() failed.
pub fn canvas_self_test() -> Option<(Canvas, CanvasSmoke)> {
    let mut canvas = Canvas::new()?;
    canvas.clear(BLACK);
    canvas.rect(0, 0, 1280, 8, 0xFF33_99FF);
    let text_y = 40;
    canvas.text(40, text_y, "クエスト名 テスト", WHITE);
    canvas.text(40, text_y + canvas.line_height(), "ASCII: mhnxeditor 0.1", 0xFF66_FF66);
    let bar_y = text_y + canvas.line_height() * 2;
    canvas.rect(40, bar_y, 400, 8, 0xFF00_00FF);

    // Count non-black pixels through the back buffer before presenting.
    let mut lit = 0u32;
    canvas.begin_frame();
    if !canvas.back.is_null() {
        for y in 0..canvas.height.max(0) as usize {
            for x in 0..canvas.width.max(0) as usize {
                let p = unsafe {
                    canvas
                        .back
                        .add(y * canvas.stride as usize + x * 4) as *const u32
                };
                if unsafe { core::ptr::read_unaligned(p) } != BLACK {
                    lit += 1;
                }
            }
        }
    }
    canvas.present();
    let smoke = CanvasSmoke {
        lit_pixels: lit,
        width: canvas.width(),
        height: canvas.height(),
    };
    Some((canvas, smoke))
}

/// Result of canvas_self_test().
pub struct CanvasSmoke {
    pub lit_pixels: u32,
    pub width: i32,
    pub height: i32,
}
