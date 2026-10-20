//! Switch front-end: SD card I/O + button input on top of the shared editor
//! state machine in mhnx_core::hbedit.
//!
//! Everything that decides *what* happens lives in hbedit, which is tested on
//! the host. This file only:
//!
//!   * scans the SD card
//!   * turns libnx button presses into hbedit::Key values
//!   * answers read/write/text requests
//!   * draws the lines hbedit returns

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void, CStr};

use mhnx_core::hbedit::{Editor, Key, Request, Write};

use crate::sys::{
    self, FsDir, FsDirectoryEntry, FsFileSystem, FS_DIR_OPEN_MODE_READ_DIRS,
    FS_DIR_OPEN_MODE_READ_FILES, HID_NPAD_STYLE_SET_NPAD_STANDARD, PAD_ANY_ID_MASK,
};

extern "C" {
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    fn fclose(stream: *mut c_void) -> c_int;
}

// HidNpadButton bits.
const BTN_A: u64 = 1 << 0;
const BTN_B: u64 = 1 << 1;
const BTN_X: u64 = 1 << 2;
const BTN_Y: u64 = 1 << 3;
const BTN_L: u64 = 1 << 6;
const BTN_R: u64 = 1 << 7;
const BTN_PLUS: u64 = 1 << 10;
const BTN_MINUS: u64 = 1 << 11;
const BTN_LEFT: u64 = 1 << 12;
const BTN_UP: u64 = 1 << 13;
const BTN_RIGHT: u64 = 1 << 14;
const BTN_DOWN: u64 = 1 << 15;

/// Folders scanned for content.
const SAVE_DIRS: [&CStr; 3] = [
    c"/switch/mhnxeditor/saves",
    c"/switch/mhnxeditor",
    c"/mhnxeditor/saves",
];
const QUEST_DIRS: [&CStr; 5] = [
    c"/switch/mhnxeditor/mhxx/quest/arc", // where extract_quests writes
    c"/switch/mhnxeditor/quest/arc",
    c"/switch/mhnxeditor/quest",
    c"/switch/mhnxeditor/quests",
    c"/mhnxeditor/quest",
];

/// Where quest arcs are written when extracting from a save.
///
/// The user asked for a folder beside the .nro, so the tree is
/// <sd>/switch/mhnxeditor/mhxx/quest/arc/ - one place for everything the tool
/// produces, with room for other kinds of output later.
pub const OUT_QUEST_DIR: &str = "/switch/mhnxeditor/mhxx/quest/arc";

/// Repeating a held button would be nicer, but edge triggered keeps the code
/// simple and predictable. This many frames must pass before a repeat.
const REPEAT_DELAY: u32 = 12;

// ---------------------------------------------------------------------------
// file helpers
// ---------------------------------------------------------------------------

fn to_cstring(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let p = to_cstring(path);
    let mode = to_cstring("rb");
    let f = unsafe { fopen(p.as_ptr() as *const c_char, mode.as_ptr() as *const c_char) };
    if f.is_null() {
        return None;
    }
    let mut out: Vec<u8> = Vec::new();
    let mut buf = alloc::vec![0u8; 64 * 1024];
    loop {
        let n = unsafe { fread(buf.as_mut_ptr() as *mut c_void, 1, buf.len(), f) };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > 12 * 1024 * 1024 {
            break;
        }
    }
    unsafe { fclose(f) };
    Some(out)
}

fn write_file(path: &str, data: &[u8]) -> bool {
    let p = to_cstring(path);
    let mode = to_cstring("wb");
    let f = unsafe { fopen(p.as_ptr() as *const c_char, mode.as_ptr() as *const c_char) };
    if f.is_null() {
        return false;
    }
    let n = unsafe { fwrite(data.as_ptr() as *const c_void, 1, data.len(), f) };
    unsafe { fclose(f) };
    n == data.len()
}

extern "C" {
    fn mkdir(path: *const c_char, mode: u32) -> i32;
}

/// Create a directory, ignoring "already exists".
fn make_dir(path: &str) -> bool {
    let p = to_cstring(path);
    unsafe { mkdir(p.as_ptr() as *const c_char, 0o777) == 0 }
}

/// Create every component of a path under /switch.
///
/// libnx has no recursive mkdir, so walk the components and create each one.
/// An existing directory reports failure through mkdir, which is not an error
/// here, so the return value only says "we tried"; extract_quests decides
/// success by whether the file actually lands.
fn make_dirs(path: &str) {
    let mut cur = String::new();
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        cur.push('/');
        cur.push_str(part);
        make_dir(&cur);
    }
}

/// Write the quest arcs held in the open save out as files.
///
/// Returns how many were written. The payload is copied byte for byte: the
/// save already holds a finished arc, so nothing is re-packed.
pub fn extract_quests(save: &mhnx_core::save::Save, ids: &[i32]) -> usize {
    use mhnx_core::dlc;
    let list = dlc::list_quests(save);
    if list.is_empty() {
        return 0;
    }
    make_dirs(OUT_QUEST_DIR);
    let mut written = 0usize;
    let mut failed = 0usize;
    for q in &list {
        if !ids.is_empty() && !ids.contains(&q.quest_id) {
            continue;
        }
        if q.data.is_empty() {
            continue;
        }
        let path = format!("{}/q{:07}.arc", OUT_QUEST_DIR, q.quest_id);
        if write_file(&path, &q.data) {
            written += 1;
        } else {
            failed += 1;
        }
    }
    // A failed write is not "no matching quest": say which it was.
    LAST_EXTRACT_FAILED.store(failed, core::sync::atomic::Ordering::Relaxed);
    written
}

/// Writes that failed during the last extract, for the status line.
pub static LAST_EXTRACT_FAILED: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

struct Entry {
    name: String,
    size: i64,
    is_file: bool,
}

fn list_dir(path: &CStr) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut fs = FsFileSystem::ZERO;
    let rc = unsafe { sys::fsOpenSdCardFileSystem(&mut fs) };
    if !sys::r_succeeded(rc) {
        return out;
    }
    let mut dir = FsDir::ZERO;
    let rc = unsafe {
        sys::fsFsOpenDirectory(
            &mut fs,
            path.as_ptr(),
            FS_DIR_OPEN_MODE_READ_DIRS | FS_DIR_OPEN_MODE_READ_FILES,
            &mut dir,
        )
    };
    if sys::r_succeeded(rc) {
        loop {
            let mut entries = [FsDirectoryEntry::ZERO; 32];
            let mut total = 0i64;
            let rc =
                unsafe { sys::fsDirRead(&mut dir, &mut total, entries.len(), entries.as_mut_ptr()) };
            if !sys::r_succeeded(rc) {
                break;
            }
            let mut any = false;
            for e in entries.iter() {
                if e.is_empty() {
                    continue;
                }
                any = true;
                out.push(Entry {
                    name: String::from(e.name_str()),
                    size: e.file_size,
                    is_file: e.is_file(),
                });
            }
            if !any || (total as usize) <= out.len() {
                break;
            }
        }
        unsafe { sys::fsDirClose(&mut dir) };
    }
    unsafe { sys::fsFsClose(&mut fs) };
    out
}

/// Collect save candidates from every candidate folder.
fn scan_saves() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in SAVE_DIRS.iter() {
        let dir = d.to_str().unwrap_or("/");
        for e in list_dir(d) {
            if !e.is_file {
                continue;
            }
            let known_size = matches!(e.size, 4_726_152 | 4_726_188 | 5_159_100);
            if !known_size && e.name != "system" && e.name != "system_backup" {
                continue;
            }
            // Never offer the backup copy: writing it would break the pair rule.
            if e.name.ends_with("_backup") {
                continue;
            }
            let full = format!("{}/{}", dir, e.name);
            if !out.contains(&full) {
                out.push(full);
            }
        }
    }
    out
}

/// Collect .arc quest files from every candidate folder.
fn scan_quests() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in QUEST_DIRS.iter() {
        let dir = d.to_str().unwrap_or("/");
        for e in list_dir(d) {
            if !e.is_file || !e.name.ends_with(".arc") {
                continue;
            }
            let full = format!("{}/{}", dir, e.name);
            if !out.contains(&full) {
                out.push(full);
            }
        }
    }
    // Numeric order so q1010001 comes before q1010002.
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------

/// Draws the editor through whichever back end is available.
///
/// The Japanese-capable canvas is preferred; until it is available this falls
/// back to libnx' ASCII console so the editor is always usable.
struct Painter {
    canvas: Option<crate::text::Canvas>,
}

impl Painter {
    fn new() -> Self {
        Painter {
            canvas: crate::text::Canvas::new(),
        }
    }

    fn draw(&mut self, ed: &Editor) {
        let lines = ed.lines();
        match &mut self.canvas {
            None => {
                unsafe { sys::consoleClear() };
                sys::log_line("mhnxeditor - MHXX editor (Switch)");
                sys::log_line("=====================================================");
                for l in &lines {
                    sys::log_line(&format!("{}{}", if l.cursor { ">" } else { " " }, l.text));
                }
                sys::log_line("");
                sys::log_line(&format!(">> {}", ed.message));
                sys::log_line(hint(ed.screen));
                unsafe { sys::consoleUpdate(core::ptr::null_mut()) };
            }
            Some(c) => {
                // Header, rows, footer. Depths are chosen for a 1280x720 panel.
                let w = c.width();
                let h = c.height();
                c.clear(0xFF10_1014);
                c.rect(0, 0, w, 40, 0xFF20_2430);
                c.text(12, 10, "mhnxeditor", 0xFFFF_FFFF);
                let mut y = 52;
                for l in &lines {
                    let color = if l.cursor {
                        0xFF60_D0FF
                    } else if l.selectable {
                        0xFFE0_E0E0
                    } else {
                        0xFFA0_A0A8
                    };
                    let prefix = if l.cursor { "> " } else { "  " };
                    c.text(12, y, &format!("{}{}", prefix, l.text), color);
                    y += 22;
                    if y > h - 90 {
                        break;
                    }
                }
                c.rect(0, h - 76, w, 76, 0xFF20_2430);
                c.text(12, h - 68, &ed.message, 0xFFFF_D070);
                c.text(12, h - 44, hint(ed.screen), 0xFFB0_B0B8);
                c.present();
            }
        }
    }
}

/// Context sensitive button hint for the current screen.
fn hint(screen: mhnx_core::hbedit::Screen) -> &'static str {
    use mhnx_core::hbedit::Screen::*;
    match screen {
        SaveList => "A open   Y quest editor   X options   - rescan   + exit",
        Slots => "Up/Down slot  A character  X items  L charms  R palico  Y quest  B back",
        Char => "Up/Down field  Left/Right value  X step  A save  B back",
        Items => "Up/Down item  L/R count  R pick item  X step  Y filter  A save  B back",
        ArcList => "A open quest   X quests in save   Y save editor   + exit",
        SaveQuests => "A edit  - extract this  R extract all  B back",
        ItemPick => "A choose  Y filter  X clear  B back",
        ArcEntries => "A edit  Y view  X save  - extract  R all  L check  B back",
        EntryView => "B back",
        Mib => "Up/Down field  Left/Right value  X step  A write .arc  B back",
        Gmd => "A edit text  X write .arc  B back",
        Check => "A fix  X write .arc  B back",
        Charms => "A delete  Y new/edit  X write save  B back",
        CharmEdit => "Up/Down row  L/R value  Y pick skill  X step  A write  B back",
        SkillPick => "A choose  Y type  X clear  B back",
        Palico => "A add/remove  L R language  X write save  B back",
        Options => "Up/Down row  Left/Right value  A write save  B back",
    }
}

// ---------------------------------------------------------------------------
// main loop
// ---------------------------------------------------------------------------

fn key_from(buttons: u64) -> Option<Key> {
    if buttons & BTN_UP != 0 {
        Some(Key::Up)
    } else if buttons & BTN_DOWN != 0 {
        Some(Key::Down)
    } else if buttons & BTN_LEFT != 0 {
        Some(Key::Left)
    } else if buttons & BTN_RIGHT != 0 {
        Some(Key::Right)
    } else if buttons & BTN_A != 0 {
        Some(Key::A)
    } else if buttons & BTN_B != 0 {
        Some(Key::B)
    } else if buttons & BTN_X != 0 {
        Some(Key::X)
    } else if buttons & BTN_Y != 0 {
        Some(Key::Y)
    } else if buttons & BTN_L != 0 {
        Some(Key::L)
    } else if buttons & BTN_R != 0 {
        Some(Key::R)
    } else if buttons & BTN_MINUS != 0 {
        Some(Key::Minus)
    } else if buttons & BTN_PLUS != 0 {
        Some(Key::Plus)
    } else {
        None
    }
}

/// Answer everything the editor asked for.
///
/// Returns true when the screen must be fully redrawn even if nothing else
/// changed - which is the case after the software keyboard applet has been
/// shown, because it takes over the display.
fn service(ed: &mut Editor) -> bool {
    let mut force_redraw = false;
    if let Some(req) = ed.take_request() {
        match req {
            Request::ReadSave(i) => {
                let path = ed.save_files.get(i).cloned();
                if let Some(p) = path {
                    match read_file(&p) {
                        Some(b) => ed.open_save_bytes(&p, &b),
                        None => ed.message = format!("cannot read {}", p),
                    }
                }
            }
            Request::ReadArc(i) => {
                let path = ed.arc_files.get(i).cloned();
                if let Some(p) = path {
                    match read_file(&p) {
                        Some(b) => ed.open_arc_bytes(&p, &b),
                        None => ed.message = format!("cannot read {}", p),
                    }
                }
            }
            Request::ExtractQuests { ids } => {
                let written = match &ed.save {
                    Some(s) => extract_quests(s, &ids),
                    None => {
                        ed.message = String::from("open a save first");
                        0
                    }
                };
                let failed = LAST_EXTRACT_FAILED.load(core::sync::atomic::Ordering::Relaxed);
                ed.apply_extracted(written, OUT_QUEST_DIR);
                if failed > 0 {
                    ed.message = format!("{} - {} write(s) FAILED (folder missing?)", ed.message, failed);
                }
                // The folder is new, so refresh the quest list for the next scan.
                ed.set_arc_files(scan_quests());
                force_redraw = true;
            }
        }
    }

    if let Some(w) = ed.take_write() {
        match w {
            Write::File { path, bytes } => {
                let n = bytes.len();
                if write_file(&path, &bytes) {
                    ed.message = format!("wrote {} bytes to {}", n, path);
                    // Keep the Switch save pair consistent.
                    if path.ends_with("/system") {
                        let backup = format!("{}_backup", path);
                        if let Some(body) = save_body_of(ed) {
                            let mut hdr =
                                mhnx_core::save::switch_header("system_backup", body.len()).to_vec();
                            hdr.extend_from_slice(&body);
                            let _ = write_file(&backup, &hdr);
                        }
                    }
                } else {
                    ed.message = String::from("WRITE FAILED (SD card write protected?)");
                }
            }
        }
    }

    if let Some((idx, initial)) = ed.take_text_request() {
        force_redraw = true;
        let header = if idx == mhnx_core::hbedit::TEXT_NAME {
            "キャラクター名"
        } else if idx == mhnx_core::hbedit::TEXT_ITEM_FILTER {
            "アイテム名で絞り込み"
        } else if idx == mhnx_core::hbedit::TEXT_SKILL_FILTER {
            "スキル名で絞り込み"
        } else {
            "クエストテキスト"
        };
        if !crate::swkbd::is_available() {
            ed.message = String::from("keyboard unavailable in this mode");
        } else {
            // 0 means "the largest length swkbd accepts". A limit of 60 used
            // to silently cut every quest description: all 141 shipped quests
            // have at least one string longer than that (longest 117).
            match crate::swkbd::input_text(header, &initial, 0) {
                Some(s) if idx == mhnx_core::hbedit::TEXT_NAME => {
                    // The save stores 31 bytes for a name.
                    let clipped: String = s.chars().take(31).collect();
                    ed.apply_name(clipped)
                }
                Some(s) if idx == mhnx_core::hbedit::TEXT_ITEM_FILTER => ed.apply_item_filter(s),
                Some(s) if idx == mhnx_core::hbedit::TEXT_SKILL_FILTER => ed.apply_skill_filter(s),
                Some(s) => ed.apply_text(idx, s),
                None => ed.message = String::from("input cancelled"),
            }
        }
    }

    force_redraw
}

/// The body of the currently open save, used to refresh the backup copy.
fn save_body_of(ed: &Editor) -> Option<Vec<u8>> {
    ed.save.as_ref().map(|s| s.body().to_vec())
}

/// Program entry point for the editor.
pub fn run() -> i32 {
    unsafe {
        sys::fsInitialize();
    }

    // Order matters: libnx' consoleInit() also grabs the default NWindow and
    // creates its own framebuffer, so the Canvas has to be created first.
    // Creating the Canvas afterwards would fail and silently disable Japanese
    // text. Only when the Canvas is unavailable do we fall back to the ASCII
    // console.
    let mut painter = Painter::new();
    let using_canvas = painter.canvas.is_some();
    if !using_canvas {
        unsafe { sys::consoleInit(core::ptr::null_mut()) };
    }

    let mut ed = Editor::new();
    ed.set_save_files(scan_saves());
    ed.set_arc_files(scan_quests());
    ed.message = format!(
        "{} save file(s), {} quest file(s) - press - to rescan",
        ed.save_files.len(),
        ed.arc_files.len()
    );
    if using_canvas {
        ed.message = format!("{} (Japanese text enabled)", ed.message);
    }

    let mut held: u64 = 0;
    let mut hold_frames: u32 = 0;

    unsafe {
        sys::padConfigureInput(1, HID_NPAD_STYLE_SET_NPAD_STANDARD);
        let mut pad = sys::PadState::ZERO;
        sys::padInitializeWithMask(&mut pad, PAD_ANY_ID_MASK);

        let mut need_draw = true;
        while sys::appletMainLoop() {
            sys::padUpdate(&mut pad);
            let down = pad.buttons_down();
            let cur = pad.buttons_cur;

            if down & BTN_PLUS != 0 {
                break;
            }

            // Minus means "extract this quest" on the quest-entry screen, so
            // the global rescan only fires elsewhere.
            if down & BTN_MINUS != 0 && !ed.uses_minus() {
                ed.set_save_files(scan_saves());
                ed.set_arc_files(scan_quests());
                ed.message = format!(
                    "rescan: {} save file(s), {} quest file(s)",
                    ed.save_files.len(),
                    ed.arc_files.len()
                );
                need_draw = true;
            }

            // Edge triggered, with a simple auto repeat while held.
            let mut key: Option<Key> = None;
            if cur != held {
                key = key_from(down);
                hold_frames = 0;
                held = cur;
            } else if cur != 0 && key_from(cur).is_some() {
                hold_frames += 1;
                if hold_frames >= REPEAT_DELAY {
                    hold_frames = REPEAT_DELAY * 2 / 3;
                    key = key_from(cur);
                }
            }

            if let Some(k) = key {
                ed.input(k);
                need_draw = true;
            }

            let before = ed.message.clone();
            let forced = service(&mut ed);
            if forced || ed.message != before || ed.dirty {
                need_draw = true;
            }

            if ed.quit {
                break;
            }

            if need_draw {
                painter.draw(&ed);
                need_draw = false;
            }

            sys::svcSleepThread(16_000_000);
        }

        sys::fsExit();
        if !using_canvas {
            sys::consoleExit(core::ptr::null_mut());
        }
    }

    0
}
