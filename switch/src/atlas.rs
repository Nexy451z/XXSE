//! The game's icon sheet, read from the SD card.
//!
//! This repository ships no game files, so the sheet cannot be built in. The
//! user drops the one texture the tool needs next to the .nro and it is read
//! once, on first use:
//!
//! ```text
//! /switch/XXSE/tex/HD_cmn_icon_GSM_NOMIP.tex
//! ```
//!
//! The file is `GUI/99_texture/HD_cmn_icon_GSM_NOMIP` inside
//! `nativeNX/arc/resident.arc`; the README says how to pull it out. Without it
//! the equipment box still works - it just draws the type name instead of the
//! glyph.
//!
//! Decoding is the same code the host build uses (`mhnx_core::tex`), so the
//! pixels here are the pixels the tests pinned.

use alloc::vec::Vec;
use core::ffi::c_char;

use mhnx_core::tex;

use crate::sys;

/// The file name the tool looks for.
pub const FILE_NAME: &str = "HD_cmn_icon_GSM_NOMIP.tex";

/// Every place the sheet may sit, in the order the tool looks.
///
/// The first is the documented one; the rest are where an older layout or a
/// hand copy would leave it.
const SEARCH: [&str; 5] = [
    "/switch/XXSE/tex/HD_cmn_icon_GSM_NOMIP.tex",
    "/switch/XXSE/HD_cmn_icon_GSM_NOMIP.tex",
    "/switch/mhnxeditor/tex/HD_cmn_icon_GSM_NOMIP.tex",
    "/switch/mhnxeditor/HD_cmn_icon_GSM_NOMIP.tex",
    "/HD_cmn_icon_GSM_NOMIP.tex",
];

/// Where the documented copy goes; what the panel tells the user to create.
pub const WANTED_PATH: &str = SEARCH[0];

/// The decoded sheet.
pub struct Atlas {
    /// Sheet width in pixels.
    pub width: usize,
    /// Sheet height in pixels.
    pub height: usize,
    /// RGBA8, row major.
    pub pixels: Vec<u8>,
    /// Which path it came from, so the status line can name it.
    pub source: &'static str,
}

static mut ATLAS: Option<Atlas> = None;
static mut TRIED: bool = false;

fn to_c(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let p = to_c(path);
    let m = to_c("rb");
    let f = unsafe { sys::fopen(p.as_ptr() as *const c_char, m.as_ptr() as *const c_char) };
    if f.is_null() {
        return None;
    }
    let mut out: Vec<u8> = Vec::new();
    let mut buf = alloc::vec![0u8; 64 * 1024];
    loop {
        let n = unsafe { sys::fread(buf.as_mut_ptr() as *mut core::ffi::c_void, 1, buf.len(), f) };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        // The sheet is 2 MB; anything past this is not it.
        if out.len() > 8 * 1024 * 1024 {
            unsafe { sys::fclose(f) };
            return None;
        }
    }
    unsafe { sys::fclose(f) };
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn load() -> Option<Atlas> {
    for path in SEARCH.iter() {
        let Some(bytes) = read_file(path) else {
            continue;
        };
        let Ok(img) = tex::decode_rgba8_auto(&bytes) else {
            // A file that is there but is not the sheet: keep looking rather
            // than disabling icons for the whole session.
            continue;
        };
        return Some(Atlas {
            width: img.width,
            height: img.height,
            pixels: img.pixels,
            source: path,
        });
    }
    None
}

/// The sheet, decoded on first use. `None` when the card has no copy.
pub fn get() -> Option<&'static Atlas> {
    unsafe {
        if !TRIED {
            TRIED = true;
            ATLAS = load();
        }
        ATLAS.as_ref()
    }
}

/// Forget the cached sheet and look again.
///
/// The panel offers this after telling the user where to put the file, so they
/// do not have to restart the tool.
pub fn reload() -> bool {
    unsafe {
        ATLAS = load();
        TRIED = true;
        ATLAS.is_some()
    }
}

/// True once `get` has been asked at least once.
pub fn tried() -> bool {
    unsafe { TRIED }
}
