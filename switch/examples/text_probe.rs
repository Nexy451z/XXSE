//! text_probe - compile-time + on-device check for the Japanese text stack.
//!
//! Not part of the .nro: it is a separate example binary that build.ps1 never
//! touches.  Build it with
//!
//!     cargo build --release --example text_probe
//!
//! and run dist-style through hbmenu if you want the visual part:
//!
//!   1. pl:u shared font -> stb_truetype -> coverage bitmap for every
//!      character of SAMPLE, reported as counts through libnx' console.
//!      (No framebuffer is touched, so this is also the "unit test" for the
//!      font -> glyph -> non-zero-bitmap logic.)
//!   2. consoleExit, then Canvas::new(): draws the same sample with the real
//!      renderer, counts lit pixels on the back buffer and presents it.
//!      The frame is held for five seconds so it can be eyeballed.
//!   3. Canvas dropped, console re-initialised, results printed.

#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

use alloc::format;
use core::ffi::{c_char, c_int};

use mhnx_switch::sys;
use mhnx_switch::text;

/// Kana, kanji, digits, Latin and a space: every branch of the renderer.
const SAMPLE: &str = "クエスト名 テスト 123 ABC";

#[no_mangle]
pub extern "C" fn main(_argc: c_int, _argv: *const *const c_char) -> c_int {
    unsafe { sys::consoleInit(core::ptr::null_mut()) };
    sys::log_line("mhnxeditor text probe");
    sys::log_line("=====================");

    match text::probe_shared_font(SAMPLE) {
        None => {
            sys::log_line("probe : FAILED - pl:u / shared font / stb_truetype unusable");
        }
        Some(p) => {
            sys::log_line(&format!(
                "font  : address=0x{:X} size={} bytes",
                p.font_address, p.font_size
            ));
            sys::log_line(&format!(
                "metric: ascent={} descent={} cff={}",
                p.ascent, p.descent, p.cff
            ));
            sys::log_line(&format!(
                "glyph : {} total  {} inked  {} blank  {} missing",
                p.glyphs, p.inked, p.blank, p.missing
            ));
            sys::log_line(&format!(
                "ink   : {} non-zero coverage bytes, first={:?}",
                p.nonzero_pixels, p.first_inked
            ));
            if p.inked == p.glyphs && p.nonzero_pixels > 0 {
                sys::log_line("result: OK - Japanese glyphs rasterise");
            } else {
                sys::log_line("result: WARN - some glyphs produced no ink");
            }
        }
    }
    unsafe { sys::consoleUpdate(core::ptr::null_mut()) };
    unsafe { sys::svcSleepThread(2_000_000_000) };
    unsafe { sys::consoleExit(core::ptr::null_mut()) };

    // The console owns the default window while it is initialised, so it has
    // to be gone before Canvas::new() can create its framebuffer.
    match text::canvas_self_test() {
        Some((canvas, smoke)) => {
            // The self-test frame is on screen right now; hold it.
            unsafe { sys::svcSleepThread(5_000_000_000) };
            let lit = smoke.lit_pixels;
            let (w, h) = (smoke.width, smoke.height);
            drop(canvas);

            unsafe { sys::consoleInit(core::ptr::null_mut()) };
            sys::log_line(&format!("canvas: {}x{} back buffer", w, h));
            sys::log_line(&format!("canvas: {} non-background pixels drawn", lit));
            if lit > 0 {
                sys::log_line("result: OK - framebuffer + Japanese text");
            } else {
                sys::log_line("result: FAIL - nothing was drawn");
            }
            unsafe { sys::consoleUpdate(core::ptr::null_mut()) };
            unsafe { sys::svcSleepThread(3_000_000_000) };
            unsafe { sys::consoleExit(core::ptr::null_mut()) };
        }
        None => {
            unsafe { sys::consoleInit(core::ptr::null_mut()) };
            sys::log_line("canvas: Canvas::new() -> None (framebuffer unavailable)");
            unsafe { sys::consoleUpdate(core::ptr::null_mut()) };
            unsafe { sys::svcSleepThread(3_000_000_000) };
            unsafe { sys::consoleExit(core::ptr::null_mut()) };
        }
    }

    0
}
