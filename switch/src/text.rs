//! Japanese-capable text and 2D GUI rendering for the Switch build.
//!
//! Pipeline:
//!
//!   pl:u shared font   (plInitialize / plGetSharedFontByType, PlSharedFontType_Standard)
//!     -> stb_truetype  (hidden behind switch/tools/font_shim.c, one C file)
//!       -> 8-bit coverage glyph cache (this file, 3 font sizes)
//!         -> libnx linear framebuffer (nwindow + framebuffer*, PIXEL_FORMAT_RGBA_8888)
//!
//! Colours are 0xAABBGGRR. On a little-endian machine those four bytes are
//! R,G,B,A in memory, which is exactly libnx RGBA8(r,g,b,a)
//! (display/framebuffer.h:12) for PIXEL_FORMAT_RGBA_8888. Use `rgb(r, g, b)`
//! and `rgba(r, g, b, a)` to construct colours cleanly in standard RGB order.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;

use crate::sys;
use crate::sys::Result as NxResult;

// ---------------------------------------------------------------------------
// libnx FFI
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

// switch/tools/font_shim.c
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

const PL_SERVICE_TYPE_USER: u32 = 0;
const PL_SHARED_FONT_TYPE_STANDARD: u32 = 0;
const PIXEL_FORMAT_RGBA_8888: u32 = 1;

const FB_WIDTH: u32 = 1280;
const FB_HEIGHT: u32 = 720;
const FB_COUNT: u32 = 2;

const FONT_HEIGHT_SMALL: f32 = 17.0;
const FONT_HEIGHT_MEDIUM: f32 = 22.0;
const FONT_HEIGHT_LARGE: f32 = 28.0;

/// 4 MiB cache holds >6000 glyphs across the 3 font sizes.
const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;

const FALLBACK_ADVANCE: i32 = 11;

// ---------------------------------------------------------------------------
// Colour & FontSize
// ---------------------------------------------------------------------------

/// A 32 bit RGBA colour in the framebuffer's native order (0xAABBGGRR).
pub type Color = u32;

/// Construct a native framebuffer colour from standard (R, G, B, A) bytes.
#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

/// Construct an opaque native framebuffer colour from standard (R, G, B) bytes.
#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    rgba(r, g, b, 0xFF)
}

/// Opaque black.
pub const BLACK: Color = rgb(0, 0, 0);
/// Opaque white.
pub const WHITE: Color = rgb(255, 255, 255);

/// Font size tier.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FontSize {
    /// 17px — captions, badges, byte offsets, notes.
    Small = 0,
    /// 22px — standard rows, sidebar items, buttons.
    Medium = 1,
    /// 28px — screen headers, large numbers, slot names.
    Large = 2,
}

impl FontSize {
    #[inline]
    fn idx(self) -> usize {
        self as usize
    }
}

// ---------------------------------------------------------------------------
// glyph cache
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct FontSlot {
    handle: *mut c_void,
    ascent: i32,
    descent: i32,
    line_gap: i32,
}

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
    pixels: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

/// A software framebuffer plus multi-size shared-font rasteriser and 2D GUI primitives.
pub struct Canvas {
    fb: Framebuffer,
    back: *mut u8,
    stride: u32,
    width: i32,
    height: i32,
    fonts: [FontSlot; 3],
    cache: BTreeMap<u32, CachedGlyph>,
    cache_bytes: usize,
}

impl Canvas {
    /// Create the canvas: pl:u session + standard shared font (3 sizes) + a linear,
    /// double-buffered framebuffer on the default window.
    pub fn new() -> Option<Self> {
        unsafe {
            if !sys::r_succeeded(plInitialize(PL_SERVICE_TYPE_USER)) {
                return None;
            }
            let mut font_data = PlFontData::ZERO;
            let rc = plGetSharedFontByType(&mut font_data, PL_SHARED_FONT_TYPE_STANDARD);
            if !sys::r_succeeded(rc) || font_data.address.is_null() || font_data.size < 1024 {
                plExit();
                return None;
            }

            let data_ptr = font_data.address as *const u8;
            let data_len = font_data.size as i32;

            let f_md = mhnx_font_create(data_ptr, data_len, FONT_HEIGHT_MEDIUM);
            if f_md.is_null() {
                plExit();
                return None;
            }
            let ascent_md = mhnx_font_ascent(f_md);
            if ascent_md <= 0 {
                mhnx_font_destroy(f_md);
                plExit();
                return None;
            }

            let f_sm = mhnx_font_create(data_ptr, data_len, FONT_HEIGHT_SMALL);
            let f_lg = mhnx_font_create(data_ptr, data_len, FONT_HEIGHT_LARGE);

            let make_slot = |f: *mut c_void, fallback: *mut c_void| -> FontSlot {
                let h = if f.is_null() { fallback } else { f };
                FontSlot {
                    handle: h,
                    ascent: mhnx_font_ascent(h),
                    descent: mhnx_font_descent(h),
                    line_gap: mhnx_font_line_gap(h),
                }
            };

            let fonts = [
                make_slot(f_sm, f_md),
                make_slot(f_md, f_md),
                make_slot(f_lg, f_md),
            ];

            let destroy_fonts = |slots: &[FontSlot; 3]| {
                let mut seen: [*mut c_void; 3] = [core::ptr::null_mut(); 3];
                for (i, s) in slots.iter().enumerate() {
                    if !s.handle.is_null() && !seen[..i].contains(&s.handle) {
                        mhnx_font_destroy(s.handle);
                    }
                    seen[i] = s.handle;
                }
            };

            let win = nwindowGetDefault();
            if win.is_null() {
                destroy_fonts(&fonts);
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
                destroy_fonts(&fonts);
                plExit();
                return None;
            }
            let rc = framebufferMakeLinear(&mut fb);
            if !sys::r_succeeded(rc) {
                framebufferClose(&mut fb);
                destroy_fonts(&fonts);
                plExit();
                return None;
            }

            Some(Canvas {
                fb,
                back: core::ptr::null_mut(),
                stride: 0,
                width: width as i32,
                height: height as i32,
                fonts,
                cache: BTreeMap::new(),
                cache_bytes: 0,
            })
        }
    }

    #[inline]
    pub fn width(&self) -> i32 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn line_height(&self) -> i32 {
        self.line_height_sized(FontSize::Medium)
    }

    pub fn line_height_sized(&self, size: FontSize) -> i32 {
        let f = &self.fonts[size.idx()];
        let h = f.ascent - f.descent + f.line_gap;
        if h > 0 {
            h
        } else {
            f.ascent + 4
        }
    }

    pub fn text_width(&mut self, s: &str) -> i32 {
        self.text_width_sized(s, FontSize::Medium)
    }

    pub fn text_width_sized(&mut self, s: &str, size: FontSize) -> i32 {
        let mut total = 0i32;
        for raw in s.chars() {
            if raw == '\n' || raw == '\r' {
                break;
            }
            let mut ch = sanitize(raw);
            let mut m = self.ensure_glyph(size, ch);
            if m.is_none() && ch != '?' {
                ch = '?';
                m = self.ensure_glyph(size, ch);
            }
            total = total.saturating_add(m.map(|g| g.advance).unwrap_or(FALLBACK_ADVANCE));
        }
        total
    }

    /// Fill the whole screen with an opaque colour.
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

    /// Draw one line of UTF-8 text at the default (Medium) font size.
    pub fn text(&mut self, x: i32, y: i32, s: &str, color: Color) {
        self.text_sized(x, y, s, color, FontSize::Medium);
    }

    /// Draw one line of UTF-8 text at the specified `FontSize`.
    pub fn text_sized(&mut self, x: i32, y: i32, s: &str, color: Color, size: FontSize) {
        self.begin_frame();
        if self.back.is_null() {
            return;
        }
        let ascent = self.fonts[size.idx()].ascent;
        let baseline = y.saturating_add(ascent);
        let mut pen = x;
        for raw in s.chars() {
            if raw == '\n' || raw == '\r' {
                break;
            }
            let mut ch = sanitize(raw);
            let mut metrics = self.ensure_glyph(size, ch);
            if metrics.is_none() && ch != '?' {
                ch = '?';
                metrics = self.ensure_glyph(size, ch);
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
                let cache_key = ((size.idx() as u32) << 24) | (ch as u32);
                if let Some(g) = self.cache.get(&cache_key) {
                    unsafe {
                        blend_glyph(back, stride, screen_w, screen_h, px, py, g, color);
                    }
                }
            }
            pen = pen.saturating_add(m.advance);
        }
    }

    /// Draw text truncated to `max_w` pixels so columns never collide.
    pub fn text_clipped(
        &mut self,
        x: i32,
        y: i32,
        max_w: i32,
        s: &str,
        color: Color,
        size: FontSize,
    ) {
        if max_w <= 0 {
            return;
        }
        let full_w = self.text_width_sized(s, size);
        if full_w <= max_w {
            self.text_sized(x, y, s, color, size);
            return;
        }
        let dots = "..";
        let dots_w = self.text_width_sized(dots, size);
        let budget = (max_w - dots_w).max(0);
        self.begin_frame();
        if self.back.is_null() {
            return;
        }
        let ascent = self.fonts[size.idx()].ascent;
        let baseline = y.saturating_add(ascent);
        let mut pen = x;
        for raw in s.chars() {
            if raw == '\n' || raw == '\r' {
                break;
            }
            let mut ch = sanitize(raw);
            let mut metrics = self.ensure_glyph(size, ch);
            if metrics.is_none() && ch != '?' {
                ch = '?';
                metrics = self.ensure_glyph(size, ch);
            }
            let adv = metrics.map(|m| m.advance).unwrap_or(FALLBACK_ADVANCE);
            if pen - x + adv > budget {
                break;
            }
            if let Some(m) = metrics {
                if m.w > 0 && m.h > 0 {
                    let back = self.back;
                    let stride = self.stride;
                    let screen_w = self.width;
                    let screen_h = self.height;
                    let px = pen.saturating_add(m.xoff);
                    let py = baseline.saturating_add(m.yoff);
                    let cache_key = ((size.idx() as u32) << 24) | (ch as u32);
                    if let Some(g) = self.cache.get(&cache_key) {
                        unsafe {
                            blend_glyph(back, stride, screen_w, screen_h, px, py, g, color);
                        }
                    }
                }
            }
            pen = pen.saturating_add(adv);
        }
        if budget > 0 {
            self.text_sized(pen, y, dots, color, size);
        }
    }

    /// Draw right-aligned text ending at `right_x`.
    pub fn text_right(&mut self, right_x: i32, y: i32, s: &str, color: Color, size: FontSize) {
        let w = self.text_width_sized(s, size);
        self.text_sized(right_x - w, y, s, color, size);
    }

    /// Draw text centered horizontally and vertically within `(x, y, w, h)`.
    pub fn text_centered(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        s: &str,
        color: Color,
        size: FontSize,
    ) {
        let tw = self.text_width_sized(s, size);
        let f = &self.fonts[size.idx()];
        let th = f.ascent - f.descent;
        let tx = x + (w - tw) / 2;
        let ty = y + (h - th) / 2 - 1;
        self.text_sized(tx, ty, s, color, size);
    }

    /// Draw a filled rectangle (supports alpha blending when alpha < 255).
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        self.begin_frame();
        if self.back.is_null() || w <= 0 || h <= 0 {
            return;
        }
        let alpha = (color >> 24) & 0xFF;
        if alpha == 0 {
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
        if alpha == 255 {
            for py in y0..y1 {
                let row = unsafe { self.back.add(py as usize * stride) } as *mut u32;
                for px in x0..x1 {
                    unsafe { core::ptr::write_unaligned(row.add(px as usize), color) };
                }
            }
        } else {
            for py in y0..y1 {
                let row = unsafe { self.back.add(py as usize * stride) } as *mut u32;
                for px in x0..x1 {
                    unsafe {
                        let dst = row.add(px as usize);
                        let old = core::ptr::read_unaligned(dst);
                        core::ptr::write_unaligned(dst, blend_pixel(old, color, alpha));
                    }
                }
            }
        }
    }

    /// Draw a vertical gradient rectangle from `top_color` to `bot_color`.
    pub fn rect_gradient_v(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        top_color: Color,
        bot_color: Color,
    ) {
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
        let denom = (h - 1).max(1) as u32;
        for py in y0..y1 {
            let t = ((py - y).clamp(0, h - 1) as u32 * 255) / denom;
            let row_col = blend_pixel(top_color, bot_color, t);
            let row = unsafe { self.back.add(py as usize * stride) } as *mut u32;
            for px in x0..x1 {
                unsafe { core::ptr::write_unaligned(row.add(px as usize), row_col) };
            }
        }
    }

    /// Draw a 1px horizontal line.
    #[inline]
    pub fn hline(&mut self, x: i32, y: i32, w: i32, color: Color) {
        self.rect(x, y, w, 1, color);
    }

    /// Draw a 1px vertical line.
    #[inline]
    pub fn vline(&mut self, x: i32, y: i32, h: i32, color: Color) {
        self.rect(x, y, 1, h, color);
    }

    /// Draw a rectangular border of `thickness` pixels.
    pub fn border(&mut self, x: i32, y: i32, w: i32, h: i32, thickness: i32, color: Color) {
        if w <= 0 || h <= 0 || thickness <= 0 {
            return;
        }
        let t = thickness.min(w / 2).min(h / 2).max(1);
        self.rect(x, y, w, t, color);
        self.rect(x, y + h - t, w, t, color);
        self.rect(x, y + t, t, h - t * 2, color);
        self.rect(x + w - t, y + t, t, h - t * 2, color);
    }

    /// Draw an anti-aliased rounded rectangle.
    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: Color) {
        let r = radius.min(w / 2).min(h / 2);
        if r <= 1 {
            self.rect(x, y, w, h, color);
            return;
        }
        self.begin_frame();
        if self.back.is_null() || w <= 0 || h <= 0 {
            return;
        }
        let alpha = (color >> 24) & 0xFF;
        if alpha == 0 {
            return;
        }

        // Fill middle horizontal band (full width, y + r .. y + h - r)
        if h > r * 2 {
            self.rect(x, y + r, w, h - r * 2, color);
        }
        // Fill top and bottom inner bands (x + r .. x + w - r)
        if w > r * 2 {
            self.rect(x + r, y, w - r * 2, r, color);
            self.rect(x + r, y + h - r, w - r * 2, r, color);
        }

        // Draw the 4 rounded corners with smooth subpixel coverage
        let r2 = r * 2;
        let inner_sq = (r2 - 1) * (r2 - 1);
        let outer_sq = (r2 + 1) * (r2 + 1);
        let band = (outer_sq - inner_sq).max(1);

        for dy in 0..r {
            let dy2 = (r - 1 - dy) * 2 + 1;
            for dx in 0..r {
                let dx2 = (r - 1 - dx) * 2 + 1;
                let d2 = dx2 * dx2 + dy2 * dy2;
                if d2 >= outer_sq {
                    continue;
                }
                let cov = if d2 <= inner_sq {
                    alpha
                } else {
                    (alpha * ((outer_sq - d2) as u32)) / (band as u32)
                };
                if cov == 0 {
                    continue;
                }
                let corners = [
                    (x + dx, y + dy),
                    (x + w - 1 - dx, y + dy),
                    (x + dx, y + h - 1 - dy),
                    (x + w - 1 - dx, y + h - 1 - dy),
                ];
                for &(px, py) in &corners {
                    self.put_pixel_blend(px, py, color, cov);
                }
            }
        }
    }

    /// Draw an anti-aliased rounded rectangle outline of `thickness` pixels.
    pub fn round_rect_border(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        thickness: i32,
        color: Color,
    ) {
        if w <= 0 || h <= 0 || thickness <= 0 {
            return;
        }
        let r = radius.min(w / 2).min(h / 2);
        if r <= 1 {
            self.border(x, y, w, h, thickness, color);
            return;
        }
        self.begin_frame();
        if self.back.is_null() {
            return;
        }
        let alpha = (color >> 24) & 0xFF;
        if alpha == 0 {
            return;
        }
        let t = thickness.max(1);
        // Straight edges
        if w > r * 2 {
            self.rect(x + r, y, w - r * 2, t, color);
            self.rect(x + r, y + h - t, w - r * 2, t, color);
        }
        if h > r * 2 {
            self.rect(x, y + r, t, h - r * 2, color);
            self.rect(x + w - t, y + r, t, h - r * 2, color);
        }

        // Corner arcs
        let ro2 = r * 2;
        let ri2 = (r - t).max(0) * 2;
        let out_max = (ro2 + 1) * (ro2 + 1);
        let out_min = (ro2 - 1).max(0) * (ro2 - 1).max(0);
        let in_max = (ri2 + 1) * (ri2 + 1);
        let in_min = (ri2 - 1).max(0) * (ri2 - 1).max(0);

        for dy in 0..r {
            let dy2 = (r - 1 - dy) * 2 + 1;
            for dx in 0..r {
                let dx2 = (r - 1 - dx) * 2 + 1;
                let d2 = dx2 * dx2 + dy2 * dy2;
                if d2 >= out_max || d2 <= in_min {
                    continue;
                }
                let cov_out = if d2 <= out_min {
                    255u32
                } else {
                    ((out_max - d2) as u32 * 255) / ((out_max - out_min).max(1) as u32)
                };
                let cov_in = if d2 >= in_max {
                    255u32
                } else {
                    ((d2 - in_min) as u32 * 255) / ((in_max - in_min).max(1) as u32)
                };
                let cov = (cov_out.min(cov_in) * alpha) / 255;
                if cov == 0 {
                    continue;
                }
                let corners = [
                    (x + dx, y + dy),
                    (x + w - 1 - dx, y + dy),
                    (x + dx, y + h - 1 - dy),
                    (x + w - 1 - dx, y + h - 1 - dy),
                ];
                for &(px, py) in &corners {
                    self.put_pixel_blend(px, py, color, cov);
                }
            }
        }
    }

    /// Draw an anti-aliased filled circle centered at `(cx, cy)` with radius `r`.
    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        if r <= 0 {
            return;
        }
        self.round_rect(cx - r, cy - r, r * 2, r * 2, r, color);
    }

    #[inline]
    fn put_pixel_blend(&mut self, px: i32, py: i32, color: Color, cov: u32) {
        if px < 0 || py < 0 || px >= self.width || py >= self.height || cov == 0 {
            return;
        }
        unsafe {
            let dst = self
                .back
                .add(py as usize * self.stride as usize + px as usize * 4)
                as *mut u32;
            if cov >= 255 {
                core::ptr::write_unaligned(dst, color | 0xFF00_0000);
            } else {
                let old = core::ptr::read_unaligned(dst);
                core::ptr::write_unaligned(dst, blend_pixel(old, color, cov));
            }
        }
    }

    /// Present the frame (framebufferEnd).
    pub fn present(&mut self) {
        if self.back.is_null() {
            return;
        }
        unsafe { framebufferEnd(&mut self.fb) };
        self.back = core::ptr::null_mut();
    }

    fn ensure_glyph(&mut self, size: FontSize, ch: char) -> Option<GlyphMetrics> {
        let cache_key = ((size.idx() as u32) << 24) | (ch as u32);
        if let Some(g) = self.cache.get(&cache_key) {
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

        let font_handle = self.fonts[size.idx()].handle;
        let (mut w, mut h, mut xoff, mut yoff, mut advance) = (0i32, 0i32, 0i32, 0i32, 0i32);
        let need = unsafe {
            mhnx_font_glyph(
                font_handle,
                ch as i32,
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
                    font_handle,
                    ch as i32,
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
        let metrics = GlyphMetrics {
            w,
            h,
            xoff,
            yoff,
            advance,
        };
        self.cache.insert(
            cache_key,
            CachedGlyph {
                w,
                h,
                xoff,
                yoff,
                advance,
                pixels,
            },
        );
        Some(metrics)
    }

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
            let mut seen: [*mut c_void; 3] = [core::ptr::null_mut(); 3];
            for (i, s) in self.fonts.iter_mut().enumerate() {
                if !s.handle.is_null() && !seen[..i].contains(&s.handle) {
                    mhnx_font_destroy(s.handle);
                }
                seen[i] = s.handle;
                s.handle = core::ptr::null_mut();
            }
            plExit();
        }
    }
}

// ---------------------------------------------------------------------------
// drawing primitives
// ---------------------------------------------------------------------------

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

pub struct FontProbe {
    pub font_address: usize,
    pub font_size: u32,
    pub ascent: i32,
    pub descent: i32,
    pub cff: bool,
    pub glyphs: u32,
    pub inked: u32,
    pub blank: u32,
    pub missing: u32,
    pub nonzero_pixels: u32,
    pub first_inked: Option<char>,
}

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
            FONT_HEIGHT_MEDIUM,
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

pub fn canvas_self_test() -> Option<(Canvas, CanvasSmoke)> {
    let mut canvas = Canvas::new()?;
    canvas.clear(BLACK);
    canvas.rect(0, 0, 1280, 8, 0xFF33_99FF);
    let text_y = 40;
    canvas.text(40, text_y, "クエスト名 テスト", WHITE);
    canvas.text(
        40,
        text_y + canvas.line_height(),
        "ASCII: XXSE 0.2",
        0xFF66_FF66,
    );
    let bar_y = text_y + canvas.line_height() * 2;
    canvas.rect(40, bar_y, 400, 8, 0xFF00_00FF);

    let mut lit = 0u32;
    canvas.begin_frame();
    if !canvas.back.is_null() {
        for y in 0..canvas.height.max(0) as usize {
            for x in 0..canvas.width.max(0) as usize {
                let p = unsafe {
                    canvas
                        .back
                        .add(y * canvas.stride as usize + x * 4)
                        as *const u32
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

pub struct CanvasSmoke {
    pub lit_pixels: u32,
    pub width: i32,
    pub height: i32,
}
