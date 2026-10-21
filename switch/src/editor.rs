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
    fn remove(path: *const c_char) -> c_int;
}

// HidNpadButton bits (services/hid.h:256-296).
const BTN_A: u64 = 1 << 0;
const BTN_B: u64 = 1 << 1;
const BTN_X: u64 = 1 << 2;
const BTN_Y: u64 = 1 << 3;
const BTN_L: u64 = 1 << 6;
const BTN_R: u64 = 1 << 7;
const BTN_ZL: u64 = 1 << 8;
const BTN_ZR: u64 = 1 << 9;
const BTN_PLUS: u64 = 1 << 10;
const BTN_MINUS: u64 = 1 << 11;
// D-Pad + Left Stick pseudo-buttons (HidNpadButton_AnyLeft/Up/Right/Down)
const BTN_LEFT: u64 = (1 << 12) | (1 << 16);
const BTN_UP: u64 = (1 << 13) | (1 << 17);
const BTN_RIGHT: u64 = (1 << 14) | (1 << 18);
const BTN_DOWN: u64 = (1 << 15) | (1 << 19);
// Right Stick Up/Down for fast page scrolling
const BTN_RSTICK_UP: u64 = 1 << 21;
const BTN_RSTICK_DOWN: u64 = 1 << 23;
// Only directional buttons auto-repeat when held so action buttons (A/B/X/Y/L/R) never misfire.
const BTN_REPEATABLE: u64 = BTN_LEFT | BTN_UP | BTN_RIGHT | BTN_DOWN;

/// Folders scanned for content.
///
/// The tool is called XXSE now, so it looks there first. The old
/// /switch/mhnxeditor paths stay in the list: files left by earlier versions
/// are still the user's saves and quests, and dropping them would look like
/// the tool had lost them.
const SAVE_DIRS: [&CStr; 5] = [
    c"/switch/XXSE/saves",
    c"/switch/XXSE",
    c"/switch/mhnxeditor/saves",
    c"/switch/mhnxeditor",
    c"/mhnxeditor/saves",
];
const QUEST_DIRS: [&CStr; 7] = [
    c"/switch/XXSE/mhxx/quest/arc", // where extract_quests writes
    c"/switch/XXSE/quest/arc",
    c"/switch/XXSE/quest",
    c"/switch/mhnxeditor/mhxx/quest/arc",
    c"/switch/mhnxeditor/quest/arc",
    c"/switch/mhnxeditor/quest",
    c"/mhnxeditor/quest",
];

/// Where quest arcs are written when extracting from a save.
///
/// The user asked for a folder beside the .nro, so the tree is
/// <sd>/switch/XXSE/mhxx/quest/arc/ - one place for everything the tool
/// produces, with room for other kinds of output later.
pub const OUT_QUEST_DIR: &str = "/switch/XXSE/mhxx/quest/arc";

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
        // A cap has to exist: a stray path pointing at a video file would
        // exhaust the heap. Returning the truncated buffer silently was worse
        // than refusing, because everything downstream then failed on a
        // half-file with no clue where it came from.
        if out.len() > 12 * 1024 * 1024 {
            unsafe { fclose(f) };
            return None;
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
    // The console's own save comes first: editing it in place is the point of
    // the tool, and the SD copies are for backups.
    out.push(String::from(mhnx_core::hbedit::CONSOLE_SAVE));
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

/// Mount the console save and read it, with a Japanese message on failure.
fn read_console_save() -> Result<Vec<u8>, String> {
    let mount = crate::savedata::Mount::open().map_err(|e| e.describe())?;
    let bytes = mount.read().map_err(|e| e.describe())?;
    if !crate::savedata::looks_like_save(&bytes) {
        return Err(String::from("セーブのサイズが想定と違います"));
    }
    Ok(bytes)
}

/// Write all 175 bundled official quest `.arc` files out to `OUT_QUEST_DIR` on the SD card.
pub fn extract_official_quests_to_sd() -> usize {
    make_dirs(OUT_QUEST_DIR);
    let mut written = 0usize;
    for q in mhnx_core::dlc_bundle::OFFICIAL_QUESTS.iter() {
        let data = mhnx_core::dlc_bundle::arc_bytes(q);
        let path = format!("{}/q{:07}.arc", OUT_QUEST_DIR, q.quest_id);
        if write_file(&path, data) {
            written += 1;
        }
    }
    written
}

/// Read an `.arc` file from `ed.arc_files[idx]` and install/overwrite it directly into `ed.save`.
pub fn install_arc_file_into_save(ed: &mut Editor, idx: usize) {
    if ed.save.is_none() {
        ed.message = String::from("先にセーブデータを開いてください");
        return;
    }
    let Some(path) = ed.arc_files.get(idx).cloned() else {
        ed.message = String::from("クエストファイルが選択されていません");
        return;
    };
    let bytes = match (&ed.arc, ed.arc_path.as_deref()) {
        (Some(arc), Some(open)) if open == path => match arc.to_bytes() {
            Ok(b) => b,
            Err(_) => {
                ed.message = String::from("アーカイブの再パックに失敗しました");
                return;
            }
        },
        _ => match read_file(&path) {
            Some(b) => b,
            None => {
                ed.message = format!("ファイルを読み込めません: {}", path);
                return;
            }
        },
    };
    let Some(id) = mhnx_core::hbedit::quest_id_of_arc(&bytes) else {
        ed.message = String::from("クエストIDが読めません（.mib が含まれていません）");
        return;
    };
    let Some(save) = &mut ed.save else {
        return;
    };
    let already = mhnx_core::dlc::list_quests(save)
        .iter()
        .any(|q| q.quest_id == id);
    let res = if already {
        mhnx_core::dlc::install_quest(save, id, &bytes).map(|slot| (slot, false))
    } else {
        mhnx_core::dlc::add_quest(save, id, &bytes).map(|slot| (slot, true))
    };
    match res {
        Ok((slot, added)) => {
            ed.dirty = true;
            ed.refresh_quest_titles();
            ed.message = if added {
                format!("クエスト {} をセーブの枠 {} に新規追加しました", id, slot)
            } else {
                format!("クエスト {} をセーブの枠 {} に上書きしました", id, slot)
            };
        }
        Err(e) => {
            ed.message = format!("クエストを追加できません: {}", e);
        }
    }
}

/// Write the save back to the console, keeping a copy on the SD card first.
fn write_console_save(bytes: &[u8]) -> Result<String, String> {
    // The console save is a header plus a 4,726,152 byte body. A 3DS save is
    // the bare body, and writing one through the in place path would leave the
    // file at its old length with the tail of the old save still on the end.
    if bytes.len() != mhnx_core::save::SWITCH_SIZE {
        return Err(alloc::format!(
            "本体セーブは {} バイトの Switch 形式だけです（受け取ったのは {} バイト）",
            mhnx_core::save::SWITCH_SIZE,
            bytes.len()
        ));
    }
    let mount = crate::savedata::Mount::open().map_err(|e| e.describe())?;
    // Keep what the console holds now, so a bad edit can be undone without
    // having thought about it beforehand.
    let backup = String::from(SAFETY_DIR);
    make_dirs(SAFETY_DIR);
    let old = mount.read().ok();
    let stamp = crate::savedata::stamp();
    let backup_path = alloc::format!("{}/system_{}.bak", backup, stamp);
    // A copy that silently failed is worse than no copy, because the message
    // afterwards claims one exists. Refuse to write the save unless the copy
    // really is on the card and reads back the same length.
    if let Some(old) = old.as_ref() {
        if !write_file(&backup_path, old) {
            return Err(alloc::format!(
                "控えを書けませんでした（{}）。セーブは変更していません",
                backup_path
            ));
        }
        prune_old_backups(10);
    } else {
        return Err(String::from(
            "いまのセーブを読み直せないため控えを作れません。セーブは変更していません",
        ));
    }
    // The write goes out in blocks and cannot be rolled back. It is refused
    // unless the copy above is on the card, so a failure half way through is
    // recoverable: say where from, in the message the user actually sees.
    if let Err(err) = mount.write_diff(bytes, old.as_deref()) {
        return Err(alloc::format!(
            "本体セーブへの書き込みに失敗しました（{}）。書き込み途中の可能性があるので、控え {} を SD カードから戻してください",
            err.describe(),
            backup_path
        ));
    }
    Ok(backup_path)
}

/// Keep at most `max_keep` backup files in `SAFETY_DIR` so the SD card never fills up.
fn prune_old_backups(max_keep: usize) {
    let entries = list_dir(c"/switch/XXSE/backups");
    let mut baks: Vec<String> = entries
        .into_iter()
        .filter(|e| e.is_file && e.name.starts_with("system_") && e.name.ends_with(".bak"))
        .map(|e| e.name)
        .collect();
    if baks.len() <= max_keep {
        return;
    }
    baks.sort();
    let remove_count = baks.len() - max_keep;
    for name in baks.into_iter().take(remove_count) {
        let full = format!("{}/{}", SAFETY_DIR, name);
        let cpath = to_cstring(&full);
        unsafe {
            let _ = remove(cpath.as_ptr() as *const c_char);
        }
    }
}

/// Where the pre-write copies of the console save are kept.
/// Where a copy of the console save is kept before it is written.
///
/// This moved with the rename. Copies made by earlier versions stay in
/// /switch/mhnxeditor/backups and are untouched; nothing reads them
/// automatically, so if one is needed it has to be restored by hand.
const SAFETY_DIR: &str = "/switch/XXSE/backups";
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
    gui: crate::ui::GuiState,
}

impl Painter {
    fn new() -> Self {
        Painter {
            canvas: crate::text::Canvas::new(),
            gui: crate::ui::GuiState::new(),
        }
    }

    fn draw(&mut self, ed: &Editor) {
        match &mut self.canvas {
            None => {
                let lines = ed.lines();
                unsafe { sys::consoleClear() };
                sys::log_line("XXSE - MHXX save editor (Switch)");
                sys::log_line("=====================================================");
                for l in &lines {
                    sys::log_line(&format!("{}{}", if l.cursor { ">" } else { " " }, l.text));
                }
                sys::log_line("");
                sys::log_line(&format!(">> {}", ed.message));
                let mut bar = alloc::string::String::new();
                for (k, label) in actions(ed.screen) {
                    if !bar.is_empty() {
                        bar.push_str("   ");
                    }
                    bar.push_str(k);
                    bar.push(' ');
                    bar.push_str(label);
                }
                sys::log_line(&bar);
                unsafe { sys::consoleUpdate(core::ptr::null_mut()) };
            }
            Some(c) => {
                self.gui.draw(c, ed);
            }
        }
    }
}

/// Screen title shown in the header.
pub(crate) fn title(screen: mhnx_core::hbedit::Screen) -> &'static str {
    use mhnx_core::hbedit::Screen::*;
    match screen {
        SaveList => "セーブを開く",
        Convert => "3DS <-> Switch 変換",
        Slots => "キャラクター選択",
        Char => "キャラクター編集",
        Items => "アイテムボックス",
        ItemPick => "アイテムを選ぶ",
        Charms => "お守り",
        CharmEdit => "お守り編集",
        SkillPick => "スキルを選ぶ",
        Equip => "装備",
        QuestEquip => "クエスト装備",
        SaveQuests => "クエスト (セーブ内)",
        ArcList => "クエスト (.arc)",
        ArcEntries => "クエストの中身",
        EntryView => "エントリ",
        Mib => "クエスト条件",
        Gmd => "クエスト本文",
        Check => "クエスト検査",
        Hunts => "狩猟記録",
        Guild => "ギルドカード",
        Roster => "オトモ",
        Sets => "マイセット",
        Palico => "配信オトモ",
        Options => "設定",
    }
}

/// The buttons that do something on this screen, as (key, label).
///
/// The footer draws these as chips, so a screen never needs the user to
/// remember a key that is not on screen.
pub(crate) fn actions(screen: mhnx_core::hbedit::Screen) -> &'static [(&'static str, &'static str)] {
    use mhnx_core::hbedit::Screen::*;
    match screen {
        SaveList => &[
            ("A", "開く"),
            ("Y", "クエスト"),
            ("L", "変換"),
            ("X", "設定"),
            ("-", "再読込"),
            ("+", "終了"),
        ],
        Convert => &[("A", "変換して保存"), ("Y", "名前"), ("B", "戻る")],
        Slots => &[
            ("A", "キャラ"),
            ("X", "アイテム"),
            ("-", "装備"),
            ("L", "お守り"),
            ("R", "オトモ"),
            ("Y", "クエスト"),
            ("B", "戻る"),
        ],
        Char => &[
            ("A", "保存"),
            ("</>", "値を変更"),
            ("X", "刻み"),
            ("-", "狩猟記録"),
            ("Y", "マイセット"),
            ("B", "戻る"),
        ],
        Items => &[
            ("R", "種類を変える"),
            ("</>", "個数"),
            ("X", "刻み"),
            ("Y", "空き表示"),
            ("L", "元に戻す"),
            ("A", "保存"),
            ("B", "戻る"),
        ],
        ItemPick => &[("A", "決定"), ("Y", "名前で絞る"), ("X", "クリア"), ("B", "戻る")],
        Charms => &[("Y", "新規/編集"), ("L", "新規"), ("A", "削除"), ("X", "保存"), ("B", "戻る")],
        CharmEdit => &[("Y", "スキル選択"), ("</>", "値"), ("A", "保存"), ("B", "戻る")],
        SkillPick => &[("A", "決定"), ("Y", "絞る"), ("X", "クリア"), ("B", "戻る")],
        Equip => &[("Y", "絞る"), ("X", "クリア"), ("L", "ギルド"), ("R", "オトモ"), ("B", "戻る")],
        QuestEquip => &[("A", "中身を開く/変更"), ("B", "戻る")],
        SaveQuests => &[
            ("A", "編集"),
            ("-", "書き出す"),
            ("R", "全部"),
            ("Y", "絞る"),
            ("L", "装備"),
            ("B", "戻る"),
        ],
        ArcList => &[("A", "開く"), ("Z", "クエスト追加"), ("X", "セーブ内"), ("Y", "セーブ編集"), ("+", "終了")],
        ArcEntries => &[
            ("A", "編集"),
            ("Y", "表示"),
            ("X", "保存"),
            ("-", "書き出す"),
            ("R", "全部"),
            ("L", "検査"),
            ("B", "戻る"),
        ],
        EntryView => &[("A", "値変更"), ("X", ".arc 保存"), ("B", "戻る")],
        Mib => &[("</>", "値変更"), ("X", "刻み"), ("A", ".arc 保存"), ("B", "戻る")],
        Gmd => &[("A", "文章編集"), ("X", ".arc 保存"), ("B", "戻る")],
        Check => &[("A", "修正"), ("X", ".arc 保存"), ("B", "戻る")],
        Hunts => &[("</>", "値"), ("X", "項目"), ("A", "反映"), ("B", "戻る")],
        Guild => &[("</>", "値"), ("X", "項目"), ("A", "コメント"), ("B", "戻る")],
        Roster => &[("</>", "値"), ("X", "項目"), ("A", "名前"), ("B", "戻る")],
        Sets => &[("B", "戻る")],
        Palico => &[("A", "追加/削除"), ("L", "言語"), ("R", "言語"), ("X", "保存"), ("B", "戻る")],
        Options => &[("</>", "値変更"), ("A", "保存"), ("B", "戻る")],
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
    } else if buttons & BTN_ZR != 0 {
        Some(Key::Z)
    } else if buttons & BTN_PLUS != 0 {
        Some(Key::Plus)
    } else {
        None
    }
}

/// Apply a completed text input (from `swkbd` or the built-in OSK) to `Editor`.
fn apply_text_result(ed: &mut Editor, idx: usize, s: String) {
    if idx == mhnx_core::hbedit::TEXT_NAME {
        let clipped: String = s.chars().take(31).collect();
        ed.apply_name(clipped);
    } else if idx == mhnx_core::hbedit::TEXT_ITEM_FILTER {
        ed.apply_item_filter(s);
    } else if idx == mhnx_core::hbedit::TEXT_SKILL_FILTER {
        ed.apply_skill_filter(s);
    } else if idx == mhnx_core::hbedit::TEXT_QUEST_FILTER {
        ed.apply_quest_filter(s);
    } else if idx == mhnx_core::hbedit::TEXT_SAVE_NAME {
        ed.apply_convert_name(s);
    } else if idx == mhnx_core::hbedit::TEXT_EQUIP_FILTER {
        ed.equip_filter = s;
        ed.equip_cursor = 0;
    } else if idx == mhnx_core::hbedit::TEXT_GUILD_COMMENT {
        if let Some(save) = &mut ed.save {
            let slot = ed.slot;
            mhnx_core::guildcard::set_comment(save, slot, &s);
            ed.dirty = true;
            ed.message = String::from("コメントを変更しました");
        }
    } else if idx >= mhnx_core::hbedit::TEXT_PALICO_NAME {
        let which = idx - mhnx_core::hbedit::TEXT_PALICO_NAME;
        if let Some(save) = &mut ed.save {
            let slot = ed.slot;
            let clipped: String = s.chars().take(20).collect();
            if mhnx_core::palico::set_name(save, slot, which, &clipped) {
                ed.dirty = true;
                ed.message = alloc::format!("オトモ {} の名前を変更しました", which);
            }
        }
    } else {
        ed.apply_text(idx, s);
    }
}

/// Answer everything the editor asked for.
///
/// Returns true when the screen must be fully redrawn even if nothing else
/// changed - which is the case after the software keyboard applet has been
/// shown, because it takes over the display.
fn service(ed: &mut Editor, gui: &mut crate::ui::GuiState) -> bool {
    let mut force_redraw = false;

    if let Some((idx, s)) = gui.osk_result.take() {
        apply_text_result(ed, idx, s);
        force_redraw = true;
    }
    if gui.osk_cancelled {
        gui.osk_cancelled = false;
        ed.message = String::from("文字入力をキャンセルしました");
        force_redraw = true;
    }
    if gui.wants_extract_official_arcs {
        gui.wants_extract_official_arcs = false;
        let written = extract_official_quests_to_sd();
        ed.set_arc_files(scan_quests());
        ed.message = format!(
            "公式イベントクエスト {} 件を {} に展開しました",
            written, OUT_QUEST_DIR
        );
        force_redraw = true;
    }
    if let Some(arc_idx) = gui.wants_install_arc_idx.take() {
        install_arc_file_into_save(ed, arc_idx);
        force_redraw = true;
    }

    if let Some(req) = ed.take_request() {
        match req {
            Request::ReadSave(i) => {
                let path = ed.save_files.get(i).cloned();
                if let Some(p) = path {
                    if p == mhnx_core::hbedit::CONSOLE_SAVE {
                        match read_console_save() {
                            Ok(b) => ed.open_save_bytes(&p, &b),
                            Err(msg) => ed.message = msg,
                        }
                    } else {
                        match read_file(&p) {
                            Some(b) => ed.open_save_bytes(&p, &b),
                            None => ed.message = format!("cannot read {}", p),
                        }
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
                // Only the two files the mount actually covers may take this
                // path. Matching every "nand:" prefix meant the convert screen,
                // which writes "nand:/system.3ds", went through here too and
                // overwrote the console save with a 3DS body.
                if path == mhnx_core::hbedit::CONSOLE_SAVE
                    || path == "nand:/system_backup"
                {
                    match write_console_save(&bytes) {
                        Ok(backup) => {
                            ed.dirty = false;
                            ed.message = format!(
                                "本体のセーブに {} バイト書き込みました（控え: {}）",
                                n, backup
                            );
                        }
                        Err(msg) => ed.message = msg,
                    }
                    ed.take_request();
                    return force_redraw;
                }
                // Keep the file we are about to replace. Nothing used to hold
                // the previous contents, so one bad write lost the save.
                let mut kept = false;
                if let Some(old) = read_file(&path) {
                    let bak = format!("{}.bak", path);
                    kept = write_file(&bak, &old);
                }
                if write_file(&path, &bytes) {
                    let is_arc = path.ends_with(".arc");
                    if !is_arc || ed.save.is_none() {
                        ed.dirty = false;
                    }
                    ed.message = if is_arc {
                        format!("クエストファイルを保存しました: {} ({} B)", path, n)
                    } else if kept {
                        format!("セーブを保存しました: {} ({} B / .bak作成済)", path, n)
                    } else {
                        format!("セーブを保存しました: {} ({} B)", path, n)
                    };
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
                    ed.message = String::from("書き込みに失敗しました（SDカードの空き容量・保護を確認してください）");
                }
            }
        }
    }

    if let Some((idx, initial)) = ed.take_text_request() {
        force_redraw = true;
        let (header, max_bytes) = if idx == mhnx_core::hbedit::TEXT_NAME {
            ("キャラクター名 (最大31B / 全角10文字)", 31usize)
        } else if idx == mhnx_core::hbedit::TEXT_ITEM_FILTER {
            ("アイテム名で絞り込み", 60)
        } else if idx == mhnx_core::hbedit::TEXT_SKILL_FILTER {
            ("スキル名で絞り込み", 60)
        } else if idx == mhnx_core::hbedit::TEXT_QUEST_FILTER {
            ("クエストIDかタイトルで絞り込み", 60)
        } else if idx == mhnx_core::hbedit::TEXT_SAVE_NAME {
            ("書き出すファイル名（拡張子は自動）", 48)
        } else if idx == mhnx_core::hbedit::TEXT_EQUIP_FILTER {
            ("装備名か種類で絞り込み", 60)
        } else if idx == mhnx_core::hbedit::TEXT_GUILD_COMMENT {
            ("ギルドカードのコメント", 54)
        } else if idx >= mhnx_core::hbedit::TEXT_PALICO_NAME
            && idx < mhnx_core::hbedit::TEXT_GUILD_COMMENT
        {
            ("オトモの名前 (最大20B / 全角6文字)", 20)
        } else {
            ("クエストテキスト", 180)
        };
        if !crate::swkbd::is_available() {
            // In Applet mode (launched from Album), Horizon OS forbids launching
            // the swkbd LibraryApplet. Use our built-in touch & controller OSK!
            gui.open_osk(idx, header, &initial, max_bytes);
        } else {
            match crate::swkbd::input_text(header, &initial, max_bytes) {
                Some(s) => apply_text_result(ed, idx, s),
                None => ed.message = String::from("文字入力をキャンセルしました"),
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
        "セーブ {} 件 / クエスト {} 件を検出しました（タッチ・スティック・ボタン対応）",
        ed.save_files.len(),
        ed.arc_files.len()
    );

    let mut held: u64 = 0;
    let mut hold_frames: u32 = 0;

    unsafe {
        sys::padConfigureInput(1, HID_NPAD_STYLE_SET_NPAD_STANDARD);
        let mut pad = sys::PadState::ZERO;
        sys::padInitializeWithMask(&mut pad, PAD_ANY_ID_MASK);
        sys::hidInitializeTouchScreen();

        let mut need_draw = true;
        while sys::appletMainLoop() {
            sys::padUpdate(&mut pad);
            let down = pad.buttons_down();
            let cur = pad.buttons_cur;

            // Action buttons are edge-triggered; only directional movement repeats when held.
            let mut key: Option<Key> = key_from(down);
            let cur_dir = cur & BTN_REPEATABLE;
            if cur_dir != held {
                hold_frames = 0;
                held = cur_dir;
            } else if key.is_none() && cur_dir != 0 {
                hold_frames += 1;
                if hold_frames >= REPEAT_DELAY {
                    hold_frames = if hold_frames > REPEAT_DELAY + 18 {
                        REPEAT_DELAY - 2
                    } else {
                        REPEAT_DELAY - 4
                    };
                    key = key_from(cur_dir);
                }
            }

            if painter.gui.osk.is_some() {
                if down & BTN_ZL != 0 {
                    key = Some(Key::L);
                } else if down & BTN_ZR != 0 {
                    key = Some(Key::R);
                }
                if let Some(k) = key {
                    if painter.gui.handle_osk_key(k) {
                        need_draw = true;
                    }
                }
            } else if painter.gui.mib_picker.is_some() {
                if down & BTN_RSTICK_UP != 0 {
                    painter.gui.scroll_mib_picker(-8);
                    need_draw = true;
                } else if down & BTN_RSTICK_DOWN != 0 {
                    painter.gui.scroll_mib_picker(8);
                    need_draw = true;
                }
                if let Some(k) = key {
                    if painter.gui.handle_mib_picker_key(&mut ed, k) {
                        need_draw = true;
                    }
                }
            } else {
                if down & BTN_PLUS != 0 {
                    break;
                }

                // ZL / ZR intuitively moves up / down through the left sidebar tabs!
                if down & BTN_ZL != 0 {
                    painter.gui.cycle_sidebar_tab(&mut ed, false);
                    need_draw = true;
                } else if down & BTN_ZR != 0 {
                    painter.gui.cycle_sidebar_tab(&mut ed, true);
                    need_draw = true;
                }

                // Right stick Up/Down scrolls lists by 8 rows
                if down & BTN_RSTICK_UP != 0 {
                    painter.gui.page_scroll(&mut ed, -8);
                    need_draw = true;
                } else if down & BTN_RSTICK_DOWN != 0 {
                    painter.gui.page_scroll(&mut ed, 8);
                    need_draw = true;
                }

                // Minus means "extract this quest" on the quest-entry screen, so
                // the global rescan only fires elsewhere.
                if (down & BTN_MINUS != 0 && !ed.uses_minus()) || painter.gui.wants_rescan {
                    painter.gui.wants_rescan = false;
                    ed.set_save_files(scan_saves());
                    ed.set_arc_files(scan_quests());
                    ed.message = format!(
                        "再スキャン完了: セーブ {} 件 / クエスト {} 件",
                        ed.save_files.len(),
                        ed.arc_files.len()
                    );
                    need_draw = true;
                }

                if let Some(k) = key {
                    // Don't double-send Key::Z when ZR was already used for tab cycling
                    if k != Key::Z {
                        painter.gui.handle_pad_key(&mut ed, k, cur);
                        need_draw = true;
                    }
                }
            }

            // Touch screen input
            let mut touch_state = sys::HidTouchScreenState::ZERO;
            let n_states = sys::hidGetTouchScreenStates(&mut touch_state, 1);
            let (touching, tx, ty) = if n_states > 0 && touch_state.count > 0 {
                let t = touch_state.touches[0];
                (true, t.x as i32, t.y as i32)
            } else {
                (false, 0, 0)
            };
            if painter.gui.handle_touch(&mut ed, touching, tx, ty) {
                need_draw = true;
            }
            if painter.gui.wants_rescan {
                painter.gui.wants_rescan = false;
                ed.set_save_files(scan_saves());
                ed.set_arc_files(scan_quests());
                ed.message = format!(
                    "再スキャン完了: セーブ {} 件 / クエスト {} 件",
                    ed.save_files.len(),
                    ed.arc_files.len()
                );
                need_draw = true;
            }

            let before = ed.message.clone();
            let forced = service(&mut ed, &mut painter.gui);
            if forced || ed.message != before {
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
